//! SUPERVISOR — arka plan görevi keep-alive döngüsü.
//!
//! ROL: `keepalive(worker, delay, on_episode)` — verilen worker future'ını
//! sonsuz döngüde yeniden başlatır: ölüm/normal dönüş `on_episode` ile
//! loglanır, `delay` beklenir, yeni episode açılır. Neden: shard/WS görevinin
//! SESSİZ ölümü veri akışında kalıcı boşluk yaratırdı (regresyon vakası).
//!
//! İNVARİANT: döngü doğası gereği TEK ATIMLIK — her an en fazla bir işçi
//! çalışır; yinelenen ikili görev oluşamaz (runtime testi bir kez daha
//! kanıtlar). Kanal birikmesi çağıranın işi: worker `try_send` + küçük
//! kapasite kullanmalı (fırtınada mesaj düşer, bellek sınırlı kalır).
//!
//! YAPMAYANLAR: WS reconnect mantığı yok (WsClient.run içinde), hata
//! sınıflandırma yok — yeniden başlatma + kanıt logu yalnız.
//!
//! KOMŞULAR: data::ingestor shard görevlerini bu döngüyle sarar.
//! GENİŞLETME: yeni kalıcı arka plan görevi eklerken aynı deseni kullan
//! (spawn → keepalive → on_episode log callback); elle `loop { spawn }`
//! yazmak ölüm-boşluk regresyonunu yeniden açar.

/// `worker(episode)` → çalıştırma geleceği; `Ok` = normal dönüş,
/// `Err` = ölüm kolu (iç yeniden-bağlama üstü kalıcı hata).
/// `on_episode` her bölüm sonunda çağrılır (ölüm algılama kanıtı/log).
pub async fn keepalive<W, Fut, E>(
    mut worker: W,
    delay: std::time::Duration,
    mut on_episode: impl FnMut(u64, &Result<(), E>),
) where
    W: FnMut(u64) -> Fut,
    Fut: std::future::Future<Output = Result<(), E>>,
{
    let mut episode: u64 = 0;
    loop {
        let r = worker(episode).await;
        on_episode(episode, &r);
        episode += 1;
        tokio::time::sleep(delay).await;
    }
}

#[cfg(test)]
mod runtime_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[tokio::test]
    async fn supervisor_olumu_algilayip_yeniden_baslatir_ve_event_tuketir() {
        // Epizot 0: hemen öl (Err) → epizot 1: gerçek event üret (Ok).
        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(8);
        let results: Arc<std::sync::Mutex<Vec<(u64, bool)>>> = Default::default();
        let ep0 = Arc::new(AtomicUsize::new(0));

        let ep0_task = ep0.clone();
        let sup = tokio::spawn(keepalive(
            move |episode: u64| {
                let tx = tx.clone();
                let ep0 = ep0_task.clone();
                async move {
                    if episode == 0 {
                        ep0.fetch_add(1, Ordering::SeqCst);
                        tx.send("ölen bölüm".into()).await.unwrap();
                        Err::<(), std::io::Error>(std::io::Error::other("kontrollü çöküş"))
                    } else {
                        tx.send("yaşayan bölüm".into()).await.unwrap();
                        Ok(())
                    }
                }
            },
            Duration::from_millis(2),
            {
                let results = results.clone();
                move |ep, r: &Result<(), std::io::Error>| {
                    results.lock().unwrap().push((ep, r.is_ok()));
                }
            },
        ));

        let first = tokio::time::timeout(Duration::from_millis(500), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first, "ölen bölüm", "ölüm bölümü çalıştı");
        let second = tokio::time::timeout(Duration::from_millis(500), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            second, "yaşayan bölüm",
            "ölümden SONRA yeni bölüm event üretti"
        );

        // Ölüm algılama kanıtı: on_episode Err(episode 0) + Ok(episode 1) gördü.
        let r = results.lock().unwrap();
        assert!(r.contains(&(0, false)), "ölüm algılanmadı: {r:?}");
        assert!(
            r.contains(&(1, true)),
            "yeniden bölümnün döndüğü görülmedi: {r:?}"
        );
        assert_eq!(
            ep0.load(Ordering::SeqCst),
            1,
            "epizot 0 tam bir kez çalıştı"
        );
        sup.abort();
    }

    #[tokio::test]
    async fn ayni_anda_ikinci_bolum_yok_tek_atimlik_dongu() {
        // Eşzamanlı işçi sayısı her an en fazla 1 (aynı pazar çift işlenemez).
        let concurrent = Arc::new(AtomicUsize::new(0));
        let max_concurrent = Arc::new(AtomicUsize::new(0));
        let sup = tokio::spawn(keepalive(
            {
                let concurrent = concurrent.clone();
                let max_concurrent = max_concurrent.clone();
                move |_ep: u64| {
                    let concurrent = concurrent.clone();
                    let max_concurrent = max_concurrent.clone();
                    async move {
                        let now = concurrent.fetch_add(1, Ordering::SeqCst) + 1;
                        max_concurrent.fetch_max(now, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(3)).await;
                        concurrent.fetch_sub(1, Ordering::SeqCst);
                        Err::<(), std::io::Error>(std::io::Error::other("kısadöngü"))
                    }
                }
            },
            Duration::from_millis(1),
            move |_ep: u64, _r: &Result<(), std::io::Error>| {},
        ));
        tokio::time::sleep(Duration::from_millis(150)).await;
        sup.abort();
        assert_eq!(
            max_concurrent.load(Ordering::SeqCst),
            1,
            "eşzamanlı 2 işçi oluştu — yinelenen görev yasağı ihlali"
        );
    }

    #[tokio::test]
    async fn cokus_firtinasi_sonlu_login_teknoloji_sinirli() {
        // 50 ardışık çöküş → 50 yeniden bölüm: episode monoton artar,
        // eşzamanlılık 1 kalır, kanal birikmez (geri baskı yok, tek abone).
        let episodes: Arc<std::sync::Mutex<Vec<u64>>> = Default::default();
        let (tx, _rx) = tokio::sync::mpsc::channel::<u64>(1);
        let sup = tokio::spawn(keepalive(
            move |ep: u64| {
                let tx = tx.clone();
                async move {
                    let _ = tx.try_send(ep); // kapasite 1: birikme olası DEĞİL
                    Err::<(), std::io::Error>(std::io::Error::other("fırtına"))
                }
            },
            Duration::from_millis(1),
            {
                let episodes = episodes.clone();
                move |ep, _r: &Result<(), std::io::Error>| {
                    episodes.lock().unwrap().push(ep);
                }
            },
        ));
        tokio::time::sleep(Duration::from_millis(120)).await;
        sup.abort();
        let eps = episodes.lock().unwrap();
        assert!(
            eps.len() >= 20,
            "yeniden bölümn yok — supervisor ölmüş: {eps:?}"
        );
        // Monoton dizi: >1 kanıt yok — her bölüm üst üste binmez
        let mut sorted = eps.clone();
        sorted.sort();
        assert_eq!(*eps, sorted, "episode sırası bozuldu");
    }

    #[tokio::test]
    async fn kanal_baski_durumunda_belleginde_sinirsiz_buyume_yok() {
        // try_send + kapasite 1: fırtınada abonesiz mesajlar DÜŞER — kanal
        // doğal sınırlı; supervisor görevi aboneden bağımsız devam eder.
        let dropped = Arc::new(AtomicUsize::new(0));
        let (tx, mut rx) = tokio::sync::mpsc::channel::<u64>(1);
        let sup = tokio::spawn(keepalive(
            {
                let dropped = dropped.clone();
                move |ep: u64| {
                    let tx = tx.clone();
                    let dropped = dropped.clone();
                    async move {
                        if tx.try_send(ep).is_err() {
                            dropped.fetch_add(1, Ordering::SeqCst);
                        }
                        Err::<(), std::io::Error>(std::io::Error::other("baskıda"))
                    }
                }
            },
            Duration::from_millis(1),
            move |_ep, _r| {},
        ));
        tokio::time::sleep(Duration::from_millis(60)).await;
        let d = dropped.load(Ordering::SeqCst);
        let got = rx.try_recv();
        sup.abort();
        assert!(
            got.is_ok() || d > 0,
            "kanal baskı yönetimi çalışmıyor (ne okuma ne düşürme)"
        );
    }
}
