// tests/data_parse.rs
use hl_monitor::data::{
    parse_clearinghouse, parse_ctx, parse_fees, parse_fills, parse_funding, parse_mids,
    parse_orders, parse_portfolio, parse_spot,
};
use serde_json::Value;

#[test]
fn parse_clearinghouse_basic() {
    let v: Value = serde_json::from_str(include_str!("fixtures/clearinghouse.json")).unwrap();
    let (ov, pos) = parse_clearinghouse(&v);
    assert!((ov.account_value - 100.5).abs() < 1e-9);
    assert!((ov.withdrawable - 90.5).abs() < 1e-9);
    assert_eq!(pos.len(), 1);
    assert_eq!(pos[0].coin, "BTC");
    assert_eq!(pos[0].side, "Long");
    assert!((pos[0].size - 0.01).abs() < 1e-9);
}

#[test]
fn parse_spot_basic() {
    let v: Value = serde_json::json!({"balances":[{"coin":"USDC","total":"10.0","hold":"2.5"},{"coin":"BTC","total":0.5,"hold":0.0}]});
    let out = parse_spot(&v);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].0, "USDC");
    assert!((out[0].1 - 10.0).abs() < 1e-9);
    assert!((out[0].2 - 2.5).abs() < 1e-9);
    assert_eq!(out[1].0, "BTC");
    assert!((out[1].1 - 0.5).abs() < 1e-9);
}

#[test]
fn parse_orders_basic() {
    let v: Value = serde_json::from_str(include_str!("fixtures/orders.json")).unwrap();
    let out = parse_orders(&v);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].oid, 91490942);
    assert_eq!(out[0].coin, "BTC");
    assert_eq!(out[0].side, "Buy");
    assert_eq!(out[0].order_type, "Limit");
    assert!((out[0].price - 29792.0).abs() < 1e-9);
    assert!((out[0].size - 5.0).abs() < 1e-9);
    assert!((out[0].orig_size - 5.0).abs() < 1e-9);
    assert!(!out[0].reduce_only);
    assert!(out[0].trigger);
    assert!((out[0].trigger_px - 30000.0).abs() < 1e-9);
    assert_eq!(out[0].cloid.as_deref(), Some("0xabc"));
    assert_eq!(out[0].timestamp, 1681247412573);
}

#[test]
fn parse_fills_basic() {
    let v: Value = serde_json::from_str(include_str!("fixtures/fills.json")).unwrap();
    let out = parse_fills(&v);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].time, 1681222254710);
    assert_eq!(out[0].coin, "BTC");
    assert_eq!(out[0].side, "B");
    assert_eq!(out[0].dir, "Close Long");
    assert!((out[0].px - 60000.0).abs() < 1e-9);
    assert!((out[0].sz - 0.01).abs() < 1e-9);
    assert!((out[0].closed_pnl - 1.5).abs() < 1e-9);
    assert!((out[0].fee - 0.1).abs() < 1e-9);
    assert_eq!(out[0].hash, "0xhash");
    assert_eq!(out[0].oid, 123);
}

#[test]
fn parse_funding_basic() {
    let v: Value = serde_json::from_str(include_str!("fixtures/funding.json")).unwrap();
    let out = parse_funding(&v);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].time, 1681222254710);
    assert_eq!(out[0].coin, "BTC");
    assert!((out[0].funding_rate - 0.0001).abs() < 1e-9);
    assert!((out[0].usdc + 0.5).abs() < 1e-9);
    assert!((out[0].szi - 0.01).abs() < 1e-9);
    assert_eq!(out[0].n_samples, 1);
}

#[test]
fn parse_fees_basic() {
    let v: Value = serde_json::json!({"userCrossRate":"0.000315","userAddRate":"0.000105","activeReferralDiscount":"0.0"});
    let f = parse_fees(&v);
    assert!((f.cross_rate - 0.000315).abs() < 1e-12);
    assert!((f.add_rate - 0.000105).abs() < 1e-12);
    assert!((f.referral_discount - 0.0).abs() < 1e-12);
}

#[test]
fn parse_portfolio_basic() {
    let v: Value = serde_json::from_str(include_str!("fixtures/portfolio.json")).unwrap();
    let p = parse_portfolio(&v);
    assert_eq!(p.periods.len(), 1);
    assert_eq!(p.periods[0].0, "day");
    assert!((p.periods[0].1 - 110.0).abs() < 1e-9);
    assert!((p.periods[0].2 - 10.0).abs() < 1e-9);
    assert!((p.periods[0].3 - 1000.0).abs() < 1e-9);
}

#[test]
fn parse_mids_basic() {
    let v: Value = serde_json::json!({"BTC":"60000.0","ETH":"3000.5"});
    let m = parse_mids(&v);
    assert_eq!(m.len(), 2);
    assert!((m["BTC"] - 60000.0).abs() < 1e-9);
    assert!((m["ETH"] - 3000.5).abs() < 1e-9);
}

#[test]
fn parse_orders_wrapped() {
    let v: Value = serde_json::from_str(r#"{"frontendOpenOrders":[{"coin":"BTC","isPositionTpsl":false,"isTrigger":true,"limitPx":"29792.0","oid":91490942,"orderType":"Limit","origSz":null,"reduceOnly":false,"side":"B","sz":"5.0","timestamp":1681247412573,"triggerCondition":"N/A","triggerPx":"30000.0","cloid":"0xabc"}]}"#).unwrap();
    let out = parse_orders(&v);
    assert_eq!(out.len(), 1);
    assert!((out[0].orig_size - 5.0).abs() < 1e-9);
}

#[test]
fn parse_fills_wrapped() {
    let v: Value = serde_json::from_str(r#"{"userFills":[{"closedPnl":"1.5","coin":"BTC","dir":"Close Long","hash":"0xhash","oid":123,"px":"60000.0","side":"B","sz":"0.01","time":1681222254710,"fee":"0.1"}]}"#).unwrap();
    let out = parse_fills(&v);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].coin, "BTC");
}

#[test]
fn parse_funding_wrapped() {
    let v: Value = serde_json::from_str(r#"{"userFunding":[{"time":1681222254710,"delta":{"type":"funding","coin":"BTC","usdc":"-0.5","szi":"0.01","fundingRate":"0.0001","nSamples":1}}]}"#).unwrap();
    let out = parse_funding(&v);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].coin, "BTC");
}

#[test]
fn parse_mids_wrapped() {
    let v: Value = serde_json::json!({"mids":{"BTC":"60000.0","ETH":"3000.5"}});
    let m = parse_mids(&v);
    assert_eq!(m.len(), 2);
    assert!((m["BTC"] - 60000.0).abs() < 1e-9);

    let v2: Value = serde_json::json!({"allMids":{"BTC":"60000.0"}});
    let m2 = parse_mids(&v2);
    assert_eq!(m2.len(), 1);
}

#[test]
fn parse_mids_flat_not_consumed() {
    // flat object with a "mids" value that is not an object? ensure flat still works
    let v: Value = serde_json::json!({"BTC":"60000.0"});
    let m = parse_mids(&v);
    assert_eq!(m.len(), 1);
}

#[test]
fn parse_ctx_basic() {
    let v: Value = serde_json::json!([
        {"universe":[{"name":"BTC"},{"name":"ETH"}]},
        [{"funding":"0.0001","openInterest":"10.0","markPx":"60000.0","oraclePx":"59999.0","dayNtlVlm":"1000000.0"},
         {"funding":"0.0002","openInterest":"20.0","markPx":"3000.0","oraclePx":"2999.0","dayNtlVlm":"2000000.0"}]
    ]);
    let c = parse_ctx(&v);
    assert_eq!(c.len(), 2);
    assert!((c["BTC"].funding - 0.0001).abs() < 1e-9);
    assert!((c["BTC"].mark_px - 60000.0).abs() < 1e-9);
    assert!((c["ETH"].open_interest - 20.0).abs() < 1e-9);
}
