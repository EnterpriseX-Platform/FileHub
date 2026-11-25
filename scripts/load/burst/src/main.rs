//! burst N RAMP BASES... — N uploads spread evenly over RAMP seconds across the
//! FileHub base URLs, one file (~200 KB) and one session each (ctx_upload.json).
use std::{collections::BTreeMap, sync::Arc, time::{Duration, Instant}};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n: usize = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(8000);
    let ramp: f64 = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(5.0);
    let bases: Vec<String> = args[3..].to_vec();
    let ctx: serde_json::Value = serde_json::from_str(&std::fs::read_to_string("ctx_upload.json").unwrap()).unwrap();
    let cookies: Arc<Vec<String>> = Arc::new(ctx["cookies"].as_array().unwrap().iter().map(|c| c.as_str().unwrap().to_string()).collect());
    let body = Arc::new(std::fs::read("upload-200k.pdf").unwrap());
    let client = reqwest::Client::builder().pool_max_idle_per_host(0).timeout(Duration::from_secs(600)).build().unwrap();
    let start = Instant::now();
    let mut tasks = Vec::with_capacity(n);
    for i in 0..n {
        let (client, cookies, body, base) = (client.clone(), cookies.clone(), body.clone(), bases[i % bases.len()].clone());
        let at = Duration::from_secs_f64(ramp * i as f64 / n as f64);
        tasks.push(tokio::spawn(async move {
            tokio::time::sleep_until(tokio::time::Instant::from_std(start + at)).await;
            let t = Instant::now();
            let form = reqwest::multipart::Form::new().text("system_id", "sys_dms")
                .part("file", reqwest::multipart::Part::bytes(body.to_vec()).file_name(format!("burst8k-{i}.pdf")));
            let r = client.post(format!("{base}/fh/api/files")).header("cookie", format!("filehub_session={}", cookies[i % cookies.len()]))
                .multipart(form).send().await;
            let code = match r { Ok(r) => r.status().as_u16().to_string(), Err(e) => if e.is_timeout() { "timeout".into() } else if e.is_connect() { "connect-error".into() } else { "io-error".into() } };
            (code, t.elapsed().as_secs_f64())
        }));
    }
    let mut codes: BTreeMap<String, usize> = BTreeMap::new();
    let mut lat = Vec::with_capacity(n);
    for t in tasks { let (c, l) = t.await.unwrap(); *codes.entry(c).or_default() += 1; lat.push(l); }
    lat.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p = |q: f64| lat[((lat.len() as f64 - 1.0) * q) as usize];
    println!("uploads={n} ramp={ramp}s wall={:.1}s", start.elapsed().as_secs_f64());
    println!("status: {codes:?}");
    println!("latency: p50={:.2}s p95={:.2}s p99={:.2}s max={:.2}s", p(0.5), p(0.95), p(0.99), p(1.0));
}
