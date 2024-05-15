use actix_web::{post, web, App, HttpRequest, HttpResponse, HttpServer};
use chrono::{TimeDelta, Utc};
use clap::{Args, Parser, Subcommand};
use directories::ProjectDirs;
use git2::{ErrorCode, Repository};
use sqlite::{Connection, State, Value};
use std::collections::HashMap;
use std::error::Error;
use std::fs;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    // Listen ip
    #[arg(short, long, default_value = "127.0.0.1")]
    listen_ip: String,

    // listen port
    #[arg(short, long, default_value_t = 8080)]
    port: u16,

    // Repository with template and cluster state
    #[arg(short, long)]
    repo: Option<String>,

    // Don't authenticate otps
    #[arg(short, long, default_value_t = false)]
    no_auth: bool,
}

#[derive(Subcommand)]
enum Commands {
    GenerateOtp(CreateOtpArgs),
    GenerateIso(CreateOtpArgs),
    CheckOtp(OtpArg),
    RemoveOtps,
}

#[derive(Args)]
struct CreateOtpArgs {
    // Number of uses before expiry
    #[arg(short, long, default_value_t = 1)]
    uses: u32,

    // Time to expiry, in hours
    #[arg(short, long, default_value_t = 12)]
    expires_in: u32,
}

#[derive(Args)]
struct OtpArg {
    // The otp
    #[arg(value_parser = clap::value_parser!(String))]
    otp: String,
}

fn connect_to_db() -> Result<Connection, Box<dyn Error>> {
    let conn = sqlite::open("otps.sqlite")?;
    let query = "CREATE TABLE IF NOT EXISTS otps (code TEXT, expires INTEGER, uses INTEGER);";
    conn.execute(query)?;
    Ok(conn)
}

async fn gen_otp(conn: &Connection, uses: u32) -> Result<String, Box<dyn Error>> {
    let id = cuid2::create_id().to_string();
    let expires = (Utc::now() + TimeDelta::hours(12)).timestamp_millis();
    let query = format!("INSERT INTO otps VALUES ('{id}', {expires}, {uses});");
    conn.execute(query)?;

    Ok(id)
}

async fn check_otp(conn: &Connection, otp: &str) -> Result<bool, Box<dyn Error>> {
    let query = "SELECT uses, expires FROM otps WHERE code = :otp";
    let mut statement = conn.prepare(query)?;
    statement.bind((":otp", otp))?;
    while let Ok(State::Row) = statement.next() {
        let uses = statement.read::<i64, _>("uses")?;
        let expires = statement.read::<i64, _>("expires")?;
        if uses > 0 && expires > Utc::now().timestamp_millis() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn remove_otps(conn: &Connection) -> Result<(), Box<dyn Error>> {
    let query = "DROP TABLE IF EXISTS otps";
    conn.execute(query)?;
    Ok(())
}

// Decrement uses left by 1 or remove from table entirely
fn use_otp(conn: &Connection, otp: &str) -> Result<(), Box<dyn Error>> {
    let query = "SELECT uses FROM otps WHERE code = :otp";
    let mut statement = conn.prepare(query)?;
    statement.bind((":otp", otp))?;
    while let Ok(State::Row) = statement.next() {
        let uses = statement.read::<i64, _>("uses")?;
        if uses > 1 {
            let query2 = "UPDATE otps SET uses = :uses WHERE code = :otp";
            let mut statement2 = conn.prepare(query2)?;
            statement2
                .bind::<&[(_, Value)]>(&[(":uses", (uses - 1).into()), (":otp", otp.into())])?;
            statement2.next()?;
        } else {
            let query2 = "DELETE FROM otps WHERE code = :otp";
            let mut statement2 = conn.prepare(query2)?;
            statement2.bind((":otp", otp))?;
            statement2.next()?;
        }
        return Ok(());
    }
    Err("Didnt find otp in db".into())
}

fn clone_repo(url: &str, dir: &str) -> Result<Repository, Box<dyn Error>> {
    match Repository::clone(url, &dir) {
        Ok(repo) => Ok(repo),
        Err(e) => {
            if &e.code() == &ErrorCode::Exists {
                Ok(Repository::open(&dir)?)
            } else {
                Err(e.into())
            }
        }
    }
}

async fn process(
    content: web::Json<HashMap<String, String>>,
    repo_url: &str,
    no_auth: bool,
) -> Result<HttpResponse, Box<dyn Error>> {
    let conn = connect_to_db()?;

    let otp = content.get("otp").unwrap();

    // Check if this client is authorized
    if !no_auth {
        let valid = check_otp(&conn, otp).await?;
        if !valid {
            return Ok(HttpResponse::Unauthorized().finish());
        }
        // decrement uses remaining
        use_otp(&conn, otp)?;
    }

    let host_key = &content.get("hostkey").unwrap();
    let configuration = rnix::Root::parse(&content.get("configuration").unwrap());
    let hardware_configuration = rnix::Root::parse(&content.get("hardware_configuration").unwrap());

    let repo_dir = ProjectDirs::from("uk", "fogbox", "server-init")
        .unwrap()
        .config_dir()
        .to_str()
        .unwrap()
        .to_owned()
        + "/repo";

    let repo = clone_repo(repo_url, &repo_dir);

    let secret_config_f = fs::read_to_string(format!("{repo_dir}/secrets/secrets.nix"))?;
    let secret_config = rnix::Root::parse(&secret_config_f);

    let cluster = secret_config
        .syntax()
        .first_child()
        .unwrap()
        .first_child()
        .unwrap();
    dbg!(cluster);
    // let cluster = secret_config
    todo!();

    Ok(HttpResponse::Ok().finish())
}

#[post("/")]
async fn init(
    content: web::Json<HashMap<String, String>>,
    _request: HttpRequest,
    repo_url: web::Data<String>,
    no_auth: web::Data<bool>,
) -> HttpResponse {
    let res = process(content, &repo_url, *no_auth.into_inner()).await;

    if res.is_ok() {
        res.unwrap()
    } else {
        HttpResponse::InternalServerError().finish()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    if cli.command.is_some() {
        let conn = connect_to_db()?;
        match cli.command.unwrap() {
            Commands::GenerateOtp(args) => {
                let otp = gen_otp(&conn, args.uses).await?;
                println!("New otp generated: {otp}");
                Ok(())
            }
            Commands::GenerateIso(args) => {
                let _otp = gen_otp(&conn, args.uses).await?;
                todo!()
            }
            Commands::CheckOtp(args) => {
                let valid = check_otp(&conn, &args.otp).await?;
                println! {"Is the otp valid?\n{valid}"};
                Ok(())
            }
            Commands::RemoveOtps => {
                remove_otps(&conn)?;
                Ok(())
            }
        }
    } else {
        if cli.repo.is_none() {
            return Err("No repository for templates or state specified".into());
        }
        let repo_url = web::Data::new(cli.repo.unwrap());
        let no_auth = web::Data::new(cli.no_auth);
        Ok(HttpServer::new(move || {
            App::new()
                .service(init)
                .app_data(repo_url.clone())
                .app_data(no_auth.clone())
        })
        .bind((cli.listen_ip, cli.port))?
        .run()
        .await?)
    }
}
