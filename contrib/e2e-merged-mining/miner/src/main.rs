//! Minimal single-threaded CPU stratum miner for the local merged-mining end-to-end test (M10 step 5c).
//! Speaks the Kaspa "large job" stratum dialect Miningcore uses by default and computes kHeavyHash with sedra-rusty's
//! own `kaspa-pow`, so its proof of work is exactly the node's. Test tool only; never point it at a real pool.
//!
//! Usage: e2e-miner <host:port> <worker login> <password> [seconds]

use kaspa_hashes::{Hash, PowHash};
use kaspa_math::Uint256;
use kaspa_pow::matrix::Matrix;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
struct Job {
    id: String,
    pre_pow_hash: Hash,
    timestamp: u64,
}

#[derive(Default)]
struct State {
    job: Option<Job>,
    job_seq: u64,
    extranonce: String,
    difficulty: f64,
    accepted: u64,
    rejected: u64,
}

fn share_target(difficulty: f64) -> Uint256 {
    // a miner on difficulty d works to (2^224 - 1) / d (Miningcore and the reference bridge)
    let max224 = (Uint256::from_u64(1) << 224) - Uint256::from_u64(1);
    if difficulty >= 1.0 {
        max224 / (difficulty.round() as u64).max(1)
    } else {
        // below 1: scale up, at most to 2^255
        let factor = (1.0 / difficulty.max(1e-9)).round() as u64;
        let t = max224 * Uint256::from_u64(factor.min(1 << 31));
        t
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (addr, login, password) = (args[1].clone(), args[2].clone(), args[3].clone());
    let seconds: u64 = args.get(4).and_then(|x| x.parse().ok()).unwrap_or(600);

    let stream = TcpStream::connect(&addr).expect("connect");
    let mut writer = stream.try_clone().unwrap();
    let state = Arc::new(Mutex::new(State { difficulty: 1.0, ..Default::default() }));

    let send = |w: &mut TcpStream, v: Value| {
        let line = v.to_string() + "\n";
        w.write_all(line.as_bytes()).unwrap();
    };
    send(&mut writer, json!({"id": 1, "method": "mining.subscribe", "params": ["e2e-cpu-miner/0.1", "EthereumStratum/1.0.0"]}));
    send(&mut writer, json!({"id": 2, "method": "mining.authorize", "params": [login, password]}));

    // reader: jobs, extranonce, difficulty, submit answers
    {
        let state = state.clone();
        let reader = BufReader::new(stream);
        std::thread::spawn(move || {
            for line in reader.lines() {
                let Ok(line) = line else { break };
                let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                let mut st = state.lock().unwrap();
                match v["method"].as_str() {
                    Some("mining.set_extranonce") => {
                        st.extranonce = v["params"][0].as_str().unwrap_or("").to_string();
                        println!("extranonce {}", st.extranonce);
                    }
                    Some("mining.set_difficulty") => {
                        st.difficulty = v["params"][0].as_f64().unwrap_or(1.0);
                        println!("difficulty {}", st.difficulty);
                    }
                    Some("mining.notify") => {
                        let id = v["params"][0].as_str().unwrap_or("").to_string();
                        let mut pre = [0u8; 32];
                        let timestamp;
                        if let Some(words) = v["params"][1].as_array() {
                            // classic dialect: [jobId, [4 x u64 little-endian words of the pre-PoW hash], timestamp]
                            for (i, w) in words.iter().take(4).enumerate() {
                                pre[8 * i..8 * i + 8].copy_from_slice(&w.as_u64().unwrap_or(0).to_le_bytes());
                            }
                            timestamp = v["params"][2].as_u64().unwrap_or(0);
                        } else {
                            // large job: [jobId, 64 hex pre-PoW hash + 16 hex little-endian timestamp]
                            let data = v["params"][1].as_str().unwrap_or("");
                            if data.len() < 80 {
                                println!("unexpected job format: {line}");
                                continue;
                            }
                            faster_hex_decode(&data[..64], &mut pre);
                            let mut ts = [0u8; 8];
                            faster_hex_decode(&data[64..80], &mut ts);
                            timestamp = u64::from_le_bytes(ts);
                        }
                        st.job = Some(Job { id, pre_pow_hash: Hash::from_bytes(pre), timestamp });
                        st.job_seq += 1;
                    }
                    _ => {
                        if let Some(id) = v["id"].as_u64() {
                            if id >= 100 {
                                if v["result"].as_bool() == Some(true) {
                                    st.accepted += 1;
                                } else {
                                    st.rejected += 1;
                                    println!("submit {id} rejected: {}", v["error"]);
                                }
                            } else {
                                println!("reply {id}: {line}");
                            }
                        }
                    }
                }
            }
            println!("connection closed");
            std::process::exit(1);
        });
    }

    let start = Instant::now();
    let mut last_report = Instant::now();
    let mut submit_id = 100u64;
    let mut hashes = 0u64;
    let mut current: Option<(u64, Job, Matrix, PowHash)> = None;
    let mut counter = 0u64;
    while start.elapsed() < Duration::from_secs(seconds) {
        let (seq, job, extranonce, difficulty) = {
            let st = state.lock().unwrap();
            match &st.job {
                Some(j) => (st.job_seq, j.clone(), st.extranonce.clone(), st.difficulty),
                None => {
                    drop(st);
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }
            }
        };
        if current.as_ref().map(|c| c.0) != Some(seq) {
            current = Some((seq, job.clone(), Matrix::generate(job.pre_pow_hash), PowHash::new(job.pre_pow_hash, job.timestamp)));
            counter = 0;
        }
        let (_, job, matrix, hasher) = current.as_ref().unwrap();
        let target = share_target(difficulty);
        let en_bits = 4 * extranonce.len() as u32;
        let prefix = if extranonce.is_empty() { 0 } else { u64::from_str_radix(&extranonce, 16).unwrap_or(0) << (64 - en_bits) };
        for _ in 0..2000 {
            counter += 1;
            let nonce = prefix | (counter & (u64::MAX >> en_bits.min(63)));
            let pow = Uint256::from_le_bytes(matrix.heavy_hash(hasher.clone().finalize_with_nonce(nonce)).as_bytes());
            hashes += 1;
            if pow <= target {
                submit_id += 1;
                let mut w = writer.try_clone().unwrap();
                send(&mut w, json!({"id": submit_id, "method": "mining.submit", "params": [login, job.id, format!("{nonce:016x}")]}));
            }
        }
        if last_report.elapsed() > Duration::from_secs(10) {
            let st = state.lock().unwrap();
            println!(
                "{:>5}s {:.1} kH/s, shares accepted {} rejected {}, job {}",
                start.elapsed().as_secs(),
                hashes as f64 / last_report.elapsed().as_secs_f64() / 1000.0,
                st.accepted,
                st.rejected,
                job.id
            );
            hashes = 0;
            last_report = Instant::now();
        }
    }
    let st = state.lock().unwrap();
    println!("done: shares accepted {} rejected {}", st.accepted, st.rejected);
}

fn faster_hex_decode(src: &str, dst: &mut [u8]) {
    for (i, b) in dst.iter_mut().enumerate() {
        *b = u8::from_str_radix(&src[2 * i..2 * i + 2], 16).unwrap_or(0);
    }
}
