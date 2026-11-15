//! Mines blocks directly against a local devnet node (template -> native nonce search -> submit), so a fresh devnet
//! has a recent tip and reports itself synced. Test tool for the merged-mining end-to-end test (M10 step 5c).
//! Usage: e2e-boot <grpc url> <pay address> <blocks>

use kaspa_addresses::Address;
use kaspa_consensus_core::header::Header;
use kaspa_grpc_client::GrpcClient;
use kaspa_rpc_core::api::rpc::RpcApi;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let client = GrpcClient::connect(args[1].clone()).await.expect("connect");
    let address = Address::try_from(args[2].as_str()).expect("address");
    let blocks: usize = args[3].parse().unwrap();

    for i in 0..blocks {
        let mut template = client.get_block_template(address.clone(), vec![]).await.expect("template");
        let header: Header = (&template.block.header).try_into().expect("header");
        let state = kaspa_pow::State::new(&header);
        let nonce = (0u64..).find(|&n| state.check_pow(n).0).unwrap();
        template.block.header.nonce = nonce;
        let result = client.submit_block(template.block, false).await;
        println!("block {i}: daa {} {:?}", header.daa_score, result.map(|r| r.report));
    }
    let info = client.get_info().await.unwrap();
    println!("synced: {}", info.is_synced);
}
