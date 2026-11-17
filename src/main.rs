use hl_monitor::run;

// manual arg parse
fn parse_args() -> hl_monitor::Args {
    let mut args = hl_monitor::Args {
        once: false,
        interval: 10,
        coins: vec!["HYPE".into(), "BTC".into(), "SOL".into(), "JUP".into(), "PURR".into()],
        fills_limit: 500,
        funding_days: 90,
    };
    let raw: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < raw.len() {
        match raw[i].as_str() {
            "--once" => {
                args.once = true;
                i += 1;
            }
            "--interval" => {
                args.interval = raw[i + 1].parse().unwrap_or(2).max(2);
                i += 2;
            }
            "--fills-limit" => {
                args.fills_limit = raw[i + 1].parse().unwrap_or(500);
                i += 2;
            }
            "--funding-days" => {
                args.funding_days = raw[i + 1].parse().unwrap_or(90);
                i += 2;
            }
            "--coins" | "--coin" => {
                for c in raw[i + 1].split(',') {
                    let c = c.trim().to_uppercase();
                    if !c.is_empty() {
                        args.coins.push(c);
                    }
                }
                i += 2;
            }
            _ => {
                i += 1;
            }
        }
    }
    args
}

fn ensure_beeps() {
    if std::path::Path::new("/tmp/opencode/beep_open.wav").exists() { return; }
    let _ = std::fs::create_dir_all("/tmp/opencode");
    let script = r#"
import wave, struct, math
def make(path, freq, dur=0.3, rate=22050):
    n = int(rate*dur); w = wave.open(path,'w'); w.setnchannels(1); w.setsampwidth(2); w.setframerate(rate)
    for i in range(n):
        t = i/rate; env = 1.0-(i/n); val = int(32767*0.5*env*math.sin(2*math.pi*freq*t)); w.writeframes(struct.pack('<h', val))
    w.close()
make('/tmp/opencode/beep_open.wav', 880)
make('/tmp/opencode/beep_close.wav', 440)
"#;
    let _ = std::process::Command::new("python3").arg("-c").arg(script).status();
}

#[tokio::main]
async fn main() {
    ensure_beeps();
    let args = parse_args();
    if let Err(e) = run(args).await {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
