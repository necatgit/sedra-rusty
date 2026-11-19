//! SEDRA (#13): native CPU miner for the public testnet (SIP-0003). Asks a node for a block template over gRPC,
//! searches a nonce with kHeavyHash on all threads, submits the block, repeats. For testnet coins, for starting a
//! fresh chain, and for the self-parent-ban test (SIP-0001 §4.5): `--sdm1` puts a well-formed `SDM1` commitment
//! into the block's own coinbase, which the node must accept before the ban start and reject after it.
//!
//! Usage: sedra-testnet-miner --rpc grpc://127.0.0.1:22210 --address sedratest:... [--blocks N] [--threads N]
//!        [--extra-data TEXT] [--sdm1]
//!
//! Testnet and devnet only: it refuses a mainnet address and a node on another network.

use kaspa_addresses::{Address, Prefix};
use kaspa_consensus_core::header::Header;
use kaspa_grpc_client::GrpcClient;
use kaspa_rpc_core::{api::rpc::RpcApi, SubmitBlockReport};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

/// A well-formed Sedra merged-mining commitment: the tag and 43 base64url characters (32 zero bytes). It commits to
/// nothing real; the node only checks whether a block's own coinbase carries one (§4.5).
const SDM1_TEST_COMMITMENT: &str = "SDM1AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

/// How long one template is worked on before a fresh one is fetched.
const TEMPLATE_BUDGET: Duration = Duration::from_secs(2);

struct Opts {
    rpc: String,
    address: Address,
    blocks: Option<u64>,
    threads: usize,
    extra_data: Vec<u8>,
}

fn usage() -> ! {
    eprintln!(
        "usage: sedra-testnet-miner --rpc grpc://HOST:PORT --address sedratest:ADDR [--blocks N] [--threads N] [--extra-data TEXT] [--sdm1]"
    );
    std::process::exit(2)
}

fn parse() -> Opts {
    let mut rpc = None;
    let mut address = None;
    let mut blocks = None;
    let mut threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let mut extra = String::new();
    let mut sdm1 = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = || args.next().unwrap_or_else(|| usage());
        match a.as_str() {
            "--rpc" => rpc = Some(val()),
            "--address" => address = Some(val()),
            "--blocks" => blocks = Some(val().parse().unwrap_or_else(|_| usage())),
            "--threads" => threads = val().parse().unwrap_or_else(|_| usage()),
            "--extra-data" => extra = val(),
            "--sdm1" => sdm1 = true,
            _ => usage(),
        }
    }
    let address = Address::try_from(address.unwrap_or_else(|| usage()).as_str()).unwrap_or_else(|e| {
        eprintln!("bad address: {e}");
        std::process::exit(2)
    });
    if !matches!(address.prefix, Prefix::Testnet | Prefix::Devnet) {
        eprintln!("refusing a {} address: this miner is for the Sedra testnet and devnet only", address.prefix);
        std::process::exit(2);
    }
    if sdm1 {
        extra.push_str(SDM1_TEST_COMMITMENT);
    }
    Opts { rpc: rpc.unwrap_or_else(|| usage()), address, blocks, threads: threads.max(1), extra_data: extra.into_bytes() }
}

/// Searches nonces on `threads` threads for at most `budget`. Returns the nonce found, if any.
fn search(header: &Header, threads: usize, budget: Duration, hashes: &AtomicU64) -> Option<u64> {
    let state = kaspa_pow::State::new(header);
    let found = AtomicU64::new(0);
    let done = AtomicBool::new(false);
    let deadline = Instant::now() + budget;
    let seed = header.timestamp.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ header.daa_score;
    std::thread::scope(|s| {
        for t in 0..threads as u64 {
            let (state, found, done) = (&state, &found, &done);
            s.spawn(move || {
                let mut nonce = seed.wrapping_add(t << 56);
                let mut n = 0u64;
                while !done.load(Ordering::Relaxed) && Instant::now() < deadline {
                    for _ in 0..1024 {
                        if state.check_pow(nonce).0 {
                            found.store(nonce, Ordering::SeqCst);
                            done.store(true, Ordering::SeqCst);
                            break;
                        }
                        nonce = nonce.wrapping_add(1);
                    }
                    n += 1024;
                }
                hashes.fetch_add(n, Ordering::Relaxed);
            });
        }
    });
    done.load(Ordering::SeqCst).then(|| found.load(Ordering::SeqCst))
}

#[tokio::main]
async fn main() {
    let o = parse();
    let client = GrpcClient::connect(o.rpc.clone()).await.unwrap_or_else(|e| {
        eprintln!("cannot connect to {}: {e}", o.rpc);
        std::process::exit(1)
    });
    let info = client.get_block_dag_info().await.expect("getBlockDagInfo");
    let network = info.network.to_string();
    println!("node {} network {network} virtual DAA score {}, {} threads", o.rpc, info.virtual_daa_score, o.threads);
    if !network.contains("testnet") && !network.contains("devnet") {
        eprintln!("refusing network {network}: testnet and devnet only");
        std::process::exit(2);
    }

    let hashes = Arc::new(AtomicU64::new(0));
    let started = Instant::now();
    let (mut accepted, mut rejected) = (0u64, 0u64);
    let mut warned_unsynced = false;
    while o.blocks.map_or(true, |b| accepted + rejected < b) {
        let mut template = match client.get_block_template(o.address.clone(), o.extra_data.clone()).await {
            Ok(t) => t,
            Err(e) => {
                eprintln!("template: {e}");
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };
        if !template.is_synced && !warned_unsynced {
            eprintln!("note: the node reports not synced (normal only while a fresh chain starts)");
            warned_unsynced = true;
        }
        let header: Header = match (&template.block.header).try_into() {
            Ok(h) => h,
            Err(e) => {
                eprintln!("template header: {e:?}");
                continue;
            }
        };
        let (threads, h2, counter) = (o.threads, header.clone(), hashes.clone());
        let nonce = tokio::task::spawn_blocking(move || search(&h2, threads, TEMPLATE_BUDGET, &counter)).await.expect("search thread");
        let Some(nonce) = nonce else { continue };
        template.block.header.nonce = nonce;
        match client.submit_block(template.block, false).await {
            Ok(r) if r.report == SubmitBlockReport::Success => {
                accepted += 1;
                println!("block accepted: daa {} nonce {nonce:#018x}", header.daa_score);
            }
            Ok(r) => {
                rejected += 1;
                println!("block REJECTED: daa {} {:?}", header.daa_score, r.report);
            }
            Err(e) => {
                rejected += 1;
                println!("block REJECTED: daa {} {e}", header.daa_score);
            }
        }
        if (accepted + rejected) % 10 == 0 {
            let rate = hashes.load(Ordering::Relaxed) as f64 / started.elapsed().as_secs_f64().max(1.0);
            println!("{accepted} accepted, {rejected} rejected, {rate:.0} hashes/s");
        }
    }
    println!("done: {accepted} accepted, {rejected} rejected");
}
