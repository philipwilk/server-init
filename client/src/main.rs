use clap::Parser;
use std::collections::HashMap;
use std::error::Error;
use std::path::Path;
use tokio::fs;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    // Url of dest server
    #[arg(short, long)]
    url: String,

    // ssh pubkey to send
    #[arg(short, long)]
    ssh_hostkey_dir: String,

    // otp to auth with
    #[arg(short, long)]
    otp: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let mut map: HashMap<&str, String> = HashMap::new();

    // Get machine's ip for ssh-ing back in
    let ip = local_ip_address::local_ip().unwrap();
    map.insert("ip", ip.to_string());

    // include otp
    map.insert("otp", args.otp);

    // get ssh hostkey file
    let hostkey = fs::read_to_string(Path::new(&args.ssh_hostkey_dir)).await?;
    map.insert("hostkey", hostkey);

    // get nix configs
    let hardware_configuration =
        fs::read_to_string(Path::new("/etc/nixos/hardware-configuration.nix")).await?;
    map.insert("hardware_configuration", hardware_configuration);
    let configuration = fs::read_to_string(Path::new("/etc/nixos/configuration.nix")).await?;
    map.insert("configuration", configuration);

    let client = reqwest::Client::new();
    let res = client.post(&args.url).json(&map).send().await?;
    let status: u16 = res.status().as_u16();

    match status {
        200 => {
            println!("Registration completed.");
            Ok(())
        }
        400 => Err("Request not formatted correctly".into()),
        401 => Err("otp not authorised: out of uses or expired".into()),
        409 => Err("pubkey already used on server: human intervention needed".into()),
        526 => Err("Destination ssl certificate rejected".into()),
        _ => Err("Other error encountered".into()),
    }
}
