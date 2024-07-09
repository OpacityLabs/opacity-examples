use dotenv::dotenv;
use elliptic_curve::pkcs8::DecodePublicKey;
use hyper::body::Buf;
use hyper::Request;
use hyper::Uri;
use reqwest::Error;

use opacity::read_env_vars;
use reqwest::ClientBuilder;
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;
use std::{str, time::Duration};
use tlsn_core::proof::{SessionProof, TlsProof};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InfoResponse {
    /// Current version of notary-server
    pub version: String,
    /// Public key of the notary signing key
    pub public_key: String,
    /// Current git commit hash of notary-server
    pub git_commit_hash: String,
    /// Current git commit timestamp of notary-server
    pub git_commit_timestamp: String,
}

/// A simple verifier which reads a proof generated by `simple_prover.rs` from "proof.json", verifies
/// it and prints the verified data to the console.
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let (notary_host, notary_port) = read_env_vars();
    let notary_public_key = notary_pubkey(notary_host, notary_port).await.unwrap();
    // Deserialize the proof
    let proof = std::fs::read_to_string("simple_proof.json").unwrap();
    let proof: TlsProof = serde_json::from_str(proof.as_str()).unwrap();

    let TlsProof {
        // The session proof establishes the identity of the server and the commitments
        // to the TLS transcript.
        session,
        // The substrings proof proves select portions of the transcript, while redacting
        // anything the Prover chose not to disclose.
        substrings,
    } = proof;

    // Verify the session proof against the Notary's public key
    //
    // This verifies the identity of the server using a default certificate verifier which trusts
    // the root certificates from the `webpki-roots` crate.
    session
        .verify_with_default_cert_verifier(notary_public_key)
        .unwrap();

    let SessionProof {
        // The session header that was signed by the Notary is a succinct commitment to the TLS transcript.
        header,
        // This is the session_info, which contains the server_name, that is checked against the
        // certificate chain shared in the TLS handshake.
        session_info,
        ..
    } = session;

    // The time at which the session was recorded
    let time = chrono::DateTime::UNIX_EPOCH + Duration::from_secs(header.time());

    // Verify the substrings proof against the session header.
    //
    // This returns the redacted transcripts
    let (mut sent, mut recv) = substrings.verify(&header).unwrap();

    // Replace the bytes which the Prover chose not to disclose with 'X'
    sent.set_redacted(b'X');
    recv.set_redacted(b'X');

    println!("-------------------------------------------------------------------");
    println!(
        "Successfully verified that the bytes below came from a session with {:?} at {}.",
        session_info.server_name, time
    );
    println!("Note that the bytes which the Prover chose not to disclose are shown as X.");
    println!();
    println!("Bytes sent:");
    println!();
    print!("{}", String::from_utf8(sent.data().to_vec()).unwrap());
    println!();
    println!("Bytes received:");
    println!();
    println!("{}", String::from_utf8(recv.data().to_vec()).unwrap());
    println!("-------------------------------------------------------------------");
}

/// Returns a Notary pubkey trusted by this Verifier
// async fn notary_pubkey() -> p256::PublicKey {
async fn notary_pubkey(notary_host: String, notary_port: u16) -> Result<p256::PublicKey, Error> {
    let url = format!("https://{}:{}/info", notary_host, notary_port);

    // Make the request
    let client = ClientBuilder::new()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();
    let response = client.get(url).send().await?;

    // Parse the response body as JSON into the ApiResponse struct
    let info_response: InfoResponse = response.json().await?;

    let public_key = p256::PublicKey::from_public_key_pem(&info_response.public_key).unwrap();

    Ok(public_key)
}
