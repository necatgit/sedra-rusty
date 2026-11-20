//! SEDRA (#13): the public testnet's measuring tool (SIP-0003 §3). Polls a list of nodes over gRPC and appends CSV
//! files that the pass report is built from (`contrib/testnet/summary.py`):
//!
//! - `nodes.csv`: per node and poll: reachable, network, virtual DAA score, sink, pruning point, synced, peers,
//!   version (criteria 1, 3, 8: who follows which chain, who stopped);
//! - `blocks.csv`: every block accepted by the reference node's selected chain, classified **native** (its own
//!   kHeavyHash meets its target) or **merged** (accepted although it does not, so through a merged-mining proof),
//!   with the coinbase's payout script and extra data (criterion 2; blocks per pool, SIP-0001 §8);
//! - `agreement.csv`: once per poll, the newest reference chain block at least `--depth` DAA deep, and whether every
//!   other node has it and has it on its selected chain (criteria 1 and 8; an old node after activation shows
//!   `present=0`).
//!
//! The first `--node` is the reference and must be an upgraded (merged-mining) node reached over gRPC. The Go node
//! sedrad v2.0.0 does not answer this gRPC client (requests time out, found in the dry run 2026-11-20), so an old Go
//! node is given as `sedractl://HOST:PORT` and asked through Go's own `sedractl` (binary from `$SEDRACTL`, default
//! `sedractl` on PATH; network flag from `$SEDRACTL_NET`, default `--testnet`). Lines starting with `ALERT` go to
//! stdout for the operator.
//!
//! Usage: sedra-testnet-monitor --out DIR --node seed1=grpc://127.0.0.1:22210 [--node old-go=sedractl://127.0.0.1:22230]...
//!        [--interval 60] [--depth 100] [--from CHAIN_BLOCK_HASH] [--once]

use kaspa_consensus_core::header::Header;
use kaspa_grpc_client::GrpcClient;
use kaspa_rpc_core::{api::rpc::RpcApi, RpcBlock, RpcHash};
use std::{
    collections::{HashSet, VecDeque},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct Node {
    name: String,
    url: String,
    client: Option<GrpcClient>,
}

/// Go `sedractl` answer as JSON, or the error it reports.
fn sedractl(server: &str, args: &[&str]) -> Result<serde_json::Value, String> {
    let bin = std::env::var("SEDRACTL").unwrap_or_else(|_| "sedractl".into());
    let net = std::env::var("SEDRACTL_NET").unwrap_or_else(|_| "--testnet".into());
    let out = std::process::Command::new(&bin)
        .arg(&net)
        .arg("-s")
        .arg(server)
        .args(args)
        .output()
        .map_err(|e| format!("{bin}: {e}"))?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("sedractl output: {e}"))?;
    let resp = v.as_object().and_then(|o| o.values().next()).cloned().ok_or("empty sedractl answer")?;
    match resp.get("error") {
        Some(e) if !e.is_null() => Err(e.get("message").and_then(|m| m.as_str()).unwrap_or("error").to_string()),
        _ => Ok(resp),
    }
}

fn js(v: &serde_json::Value, k: &str) -> String {
    match v.get(k) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

impl Node {
    fn ctl(&self) -> Option<&str> {
        self.url.strip_prefix("sedractl://")
    }

    async fn client(&mut self) -> Result<&GrpcClient, String> {
        if self.client.is_none() {
            let c = tokio::time::timeout(Duration::from_secs(10), GrpcClient::connect(self.url.clone()))
                .await
                .map_err(|_| "connect timeout".to_string())?
                .map_err(|e| e.to_string())?;
            self.client = Some(c);
        }
        Ok(self.client.as_ref().unwrap())
    }
}

struct Opts {
    out: PathBuf,
    nodes: Vec<Node>,
    interval: Duration,
    depth: u64,
    from: Option<RpcHash>,
    once: bool,
}

fn usage() -> ! {
    eprintln!("usage: sedra-testnet-monitor --out DIR --node NAME=grpc://HOST:PORT [--node ...] [--interval SECS] [--depth DAA] [--from HASH] [--once]");
    std::process::exit(2)
}

fn parse() -> Opts {
    let (mut out, mut nodes, mut interval, mut depth, mut from, mut once) = (None, Vec::new(), 60, 100, None, false);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = || args.next().unwrap_or_else(|| usage());
        match a.as_str() {
            "--out" => out = Some(PathBuf::from(val())),
            "--node" => {
                let v = val();
                let (name, url) = v.split_once('=').unwrap_or_else(|| usage());
                nodes.push(Node { name: name.to_string(), url: url.to_string(), client: None });
            }
            "--interval" => interval = val().parse().unwrap_or_else(|_| usage()),
            "--depth" => depth = val().parse().unwrap_or_else(|_| usage()),
            "--from" => from = Some(RpcHash::from_str(&val()).unwrap_or_else(|_| usage())),
            "--once" => once = true,
            _ => usage(),
        }
    }
    if nodes.is_empty() {
        usage()
    }
    Opts { out: out.unwrap_or_else(|| usage()), nodes, interval: Duration::from_secs(interval), depth, from, once }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

/// CSV-safe text: no commas, quotes or line breaks, printable ASCII only.
fn clean(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_graphic() && c != ',' && c != '"' { c } else if c == ' ' { ' ' } else { '?' }).collect()
}

fn append(dir: &Path, file: &str, header: &str, line: &str) {
    let path = dir.join(file);
    let new = !path.exists();
    let mut f = OpenOptions::new().create(true).append(true).open(&path).expect("open csv");
    if new {
        writeln!(f, "{header}").unwrap();
    }
    writeln!(f, "{line}").unwrap();
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The coinbase payload of a Kaspa-family block: blue score (8), subsidy (8), script version (2), script length (1),
/// script, then the miner's extra data.
fn coinbase_parts(payload: &[u8]) -> (String, String) {
    if payload.len() < 19 {
        return (String::new(), String::new());
    }
    let len = payload[18] as usize;
    let script = payload.get(19..19 + len).unwrap_or(&[]);
    let extra = payload.get(19 + len..).unwrap_or(&[]);
    (hex(script), clean(&String::from_utf8_lossy(extra)))
}

/// Native when the header's own kHeavyHash meets its own target; an accepted block that does not was accepted through
/// its merged-mining proof (SIP-0001 §4.4).
fn kind(block: &RpcBlock) -> &'static str {
    let header = Header::from(&block.header);
    if kaspa_pow::State::new(&header).check_pow(header.nonce).0 {
        "native"
    } else {
        "merged"
    }
}

struct State {
    cursor: Option<RpcHash>,
    seen: HashSet<RpcHash>,
    seen_order: VecDeque<RpcHash>,
}

impl State {
    fn remember(&mut self, h: RpcHash) -> bool {
        if !self.seen.insert(h) {
            return false;
        }
        self.seen_order.push_back(h);
        if self.seen_order.len() > 200_000 {
            let old = self.seen_order.pop_front().unwrap();
            self.seen.remove(&old);
        }
        true
    }
}

const NODES_HEADER: &str =
    "unix_time,node,status,network,virtual_daa_score,sink,pruning_point,block_count,is_synced,peers,server_version,error";

async fn poll_nodes(o: &mut Opts) -> Vec<Option<(u64, RpcHash)>> {
    let mut result = Vec::new();
    for node in o.nodes.iter_mut() {
        let t = now();
        if let Some(server) = node.ctl().map(str::to_string) {
            let r = (|| -> Result<(String, u64, String), String> {
                let dag = sedractl(&server, &["GetBlockDagInfo"])?;
                let info = sedractl(&server, &["GetInfo"])?;
                let peers = sedractl(&server, &["GetConnectedPeerInfo"])
                    .map(|p| p.get("infos").and_then(|i| i.as_array()).map_or(0, |a| a.len()).to_string())
                    .unwrap_or_default();
                let daa: u64 = js(&dag, "virtualDaaScore").parse().unwrap_or(0);
                let sink = dag.get("virtualParentHashes").and_then(|v| v.get(0)).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let synced = info.get("isSynced").and_then(|v| v.as_bool()).unwrap_or(false);
                let line = format!(
                    "{t},{},ok,{},{daa},{sink},{},{},{},{peers},{},",
                    node.name,
                    js(&dag, "networkName"),
                    js(&dag, "pruningPointHash"),
                    js(&dag, "blockCount"),
                    synced as u8,
                    clean(&js(&info, "serverVersion"))
                );
                Ok((line, daa, sink))
            })();
            let line = match r {
                Ok((line, daa, sink)) => {
                    result.push(Some((daa, RpcHash::from_str(&sink).unwrap_or_default())));
                    line
                }
                Err(e) => {
                    result.push(None);
                    println!("ALERT {t} node {} unreachable: {}", node.name, clean(&e));
                    format!("{t},{},error,,,,,,,,,{}", node.name, clean(&e))
                }
            };
            append(&o.out, "nodes.csv", NODES_HEADER, &line);
            continue;
        }
        let r: Result<(String, u64), String> = async {
            let c = node.client().await?;
            let dag = c.get_block_dag_info().await.map_err(|e| e.to_string())?;
            let info = c.get_info().await.map_err(|e| e.to_string())?;
            let peers = c.get_connected_peer_info().await.map(|p| p.peer_info.len().to_string()).unwrap_or_default();
            let line = format!(
                "{t},{},ok,{},{},{},{},{},{},{},{},",
                node.name,
                dag.network,
                dag.virtual_daa_score,
                dag.sink,
                dag.pruning_point_hash,
                dag.block_count,
                info.is_synced as u8,
                peers,
                clean(&info.server_version)
            );
            if !info.is_synced {
                println!("ALERT {t} node {} reports not synced (DAA {})", node.name, dag.virtual_daa_score);
            }
            result.push(Some((dag.virtual_daa_score, dag.sink)));
            Ok((line, dag.virtual_daa_score))
        }
        .await;
        let line = match r {
            Ok((line, _)) => line,
            Err(e) => {
                node.client = None;
                result.push(None);
                println!("ALERT {t} node {} unreachable: {}", node.name, clean(&e));
                format!("{t},{},error,,,,,,,,,{}", node.name, clean(&e))
            }
        };
        append(&o.out, "nodes.csv", NODES_HEADER, &line);
    }
    result
}

/// Follows the reference node's selected chain from the cursor and classifies every block each new chain block
/// accepts (its merge set). Returns the chain blocks added, oldest first.
async fn follow_chain(o: &mut Opts, st: &mut State, sink: RpcHash) -> Result<Vec<RpcBlock>, String> {
    let out = o.out.clone();
    let c = o.nodes[0].client().await?;
    let start = match st.cursor {
        Some(h) => h,
        None => {
            st.cursor = Some(sink);
            return Ok(vec![]);
        }
    };
    let vc = c.get_virtual_chain_from_block(start, false).await.map_err(|e| e.to_string())?;
    let t = now();
    for removed in &vc.removed_chain_block_hashes {
        println!("ALERT {t} reference chain reorg: {removed} left the selected chain");
    }
    let mut chain = Vec::new();
    for h in &vc.added_chain_block_hashes {
        let cb = c.get_block(*h, false).await.map_err(|e| e.to_string())?;
        let vd = cb.verbose_data.as_ref().ok_or("no verbose data")?;
        let reds: HashSet<RpcHash> = vd.merge_set_reds_hashes.iter().copied().collect();
        for m in vd.merge_set_blues_hashes.iter().chain(vd.merge_set_reds_hashes.iter()).chain(std::iter::once(h)) {
            if !st.remember(*m) {
                continue;
            }
            let b = c.get_block(*m, true).await.map_err(|e| e.to_string())?;
            let (script, extra) = b.transactions.first().map(|tx| coinbase_parts(&tx.payload)).unwrap_or_default();
            let is_chain = b.verbose_data.as_ref().map_or(false, |v| v.is_chain_block);
            append(
                &out,
                "blocks.csv",
                "unix_time,hash,daa_score,blue_score,timestamp_ms,kind,is_chain_block,red,accepted_by,payout_script,extra_data",
                &format!(
                    "{t},{m},{},{},{},{},{},{},{h},{script},{extra}",
                    b.header.daa_score,
                    b.header.blue_score,
                    b.header.timestamp,
                    kind(&b),
                    is_chain as u8,
                    reds.contains(m) as u8
                ),
            );
        }
        st.cursor = Some(*h);
        chain.push(cb);
    }
    Ok(chain)
}

/// Asks every other node about the newest recorded chain block that is at least `depth` DAA deep.
async fn check_agreement(o: &mut Opts, recent: &VecDeque<(u64, RpcHash)>, ref_daa: u64) {
    let Some(&(daa, hash)) = recent.iter().rev().find(|(d, _)| *d + o.depth <= ref_daa) else { return };
    let out = o.out.clone();
    let t = now();
    for node in o.nodes.iter_mut().skip(1) {
        if let Some(server) = node.ctl().map(str::to_string) {
            let line = match sedractl(&server, &["GetBlock", &hash.to_string(), "false"]) {
                Ok(b) => {
                    let on_chain =
                        b.pointer("/block/verboseData/isChainBlock").and_then(|v| v.as_bool()).unwrap_or(false);
                    if !on_chain {
                        println!("ALERT {t} node {} has {hash} (DAA {daa}) but not on its selected chain", node.name);
                    }
                    format!("{t},{hash},{daa},{},1,{}", node.name, on_chain as u8)
                }
                Err(e) if e.contains("not found") => {
                    println!("ALERT {t} node {} does not have reference chain block {hash} (DAA {daa})", node.name);
                    format!("{t},{hash},{daa},{},0,0", node.name)
                }
                Err(_) => continue,
            };
            append(&out, "agreement.csv", "unix_time,hash,daa_score,node,present,is_chain_block", &line);
            continue;
        }
        let line = match node.client().await {
            Ok(c) => match c.get_block(hash, false).await {
                Ok(b) => {
                    let on_chain = b.verbose_data.as_ref().map_or(false, |v| v.is_chain_block);
                    if !on_chain {
                        println!("ALERT {t} node {} has {hash} (DAA {daa}) but not on its selected chain", node.name);
                    }
                    format!("{t},{hash},{daa},{},1,{}", node.name, on_chain as u8)
                }
                Err(_) => {
                    println!("ALERT {t} node {} does not have reference chain block {hash} (DAA {daa})", node.name);
                    format!("{t},{hash},{daa},{},0,0", node.name)
                }
            },
            Err(_) => continue, // unreachable is already in nodes.csv
        };
        append(&out, "agreement.csv", "unix_time,hash,daa_score,node,present,is_chain_block", &line);
    }
}

#[tokio::main]
async fn main() {
    let mut o = parse();
    fs::create_dir_all(&o.out).expect("create --out");
    let cursor_file = o.out.join("cursor");
    let cursor = o.from.or_else(|| fs::read_to_string(&cursor_file).ok().and_then(|s| RpcHash::from_str(s.trim()).ok()));
    let mut st = State { cursor, seen: HashSet::new(), seen_order: VecDeque::new() };
    let mut recent: VecDeque<(u64, RpcHash)> = VecDeque::new();
    println!("monitoring {} nodes, reference {} ({}), output {}", o.nodes.len(), o.nodes[0].name, o.nodes[0].url, o.out.display());
    loop {
        let polled = poll_nodes(&mut o).await;
        if let Some(Some((ref_daa, sink))) = polled.first().copied() {
            match follow_chain(&mut o, &mut st, sink).await {
                Ok(chain) => {
                    for b in chain {
                        recent.push_back((b.header.daa_score, b.header.hash));
                    }
                    while recent.len() > 10_000 {
                        recent.pop_front();
                    }
                    if let Some(c) = st.cursor {
                        fs::write(&cursor_file, c.to_string()).ok();
                    }
                }
                Err(e) => {
                    o.nodes[0].client = None;
                    println!("ALERT {} following the reference chain failed: {}", now(), clean(&e));
                }
            }
            check_agreement(&mut o, &recent, ref_daa).await;
        }
        if o.once {
            break;
        }
        tokio::time::sleep(o.interval).await;
    }
}
