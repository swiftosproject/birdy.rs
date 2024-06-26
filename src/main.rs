use clap::{App, Arg, SubCommand};
use tokio::fs;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use reqwest::header::SET_COOKIE;
use reqwest::Client;
use serde::Serialize;
use std::path::Path;
use std::fs::File;
use tar::Archive;
use flate2::read::GzDecoder;

#[derive(Serialize)]
struct User {
    username: String,
    password: String,
}

#[tokio::main]
async fn main() {
    let matches = App::new("BirdyCLI")
        .version("1.0")
        .author("SwiftOS")
        .about("Package Manager for SwiftOS")
        .subcommand(
            SubCommand::with_name("register")
                .about("Registers a new user")
                .arg(Arg::with_name("username").help("The username for the new user").required(true).index(1))
                .arg(Arg::with_name("password").help("The password for the new user").required(true).index(2)),
        )
        .subcommand(
            SubCommand::with_name("login")
                .about("Login to an existing account")
                .arg(Arg::with_name("username").help("The username for the account").required(true).index(1))
                .arg(Arg::with_name("password").help("The password for the account").required(true).index(2)),
        )
        .subcommand(
            SubCommand::with_name("install")
                .about("Install a package")
                .arg(Arg::with_name("name").help("The name of the package").required(true).index(1))
                .arg(Arg::with_name("version").help("The version of the package").required(false).index(2))
                .arg(Arg::with_name("location")
                    .short('r')
                    .long("root")
                    .value_name("location")
                    .help("The Location to Install the Package to.")
                    .takes_value(true)
                    .required(false)),
        )
        .subcommand(
            SubCommand::with_name("remove")
                .about("Remove a package")
                .arg(Arg::with_name("name").help("The name of the package").required(true).index(1))
                .arg(Arg::with_name("version").help("The version of the package").required(false).index(2)),
        )
        .subcommand(
            SubCommand::with_name("list")
                .about("List all installed packages"),
        )
        .get_matches();

    if let Some(matches) = matches.subcommand_matches("register") {
        let username = matches.value_of("username").unwrap();
        let password = matches.value_of("password").unwrap();
        register(username, password).await;
    } else if let Some(matches) = matches.subcommand_matches("login") {
        let username = matches.value_of("username").unwrap();
        let password = matches.value_of("password").unwrap();
        login(username, password).await;
    } else if let Some(matches) = matches.subcommand_matches("install") {
        let name = matches.value_of("name").unwrap();
        let version = matches.value_of("version");
        let location_str = matches.value_of("location").unwrap_or("/");
        let location = Path::new(location_str);
        install(name, version, location).await;
    } else if let Some(matches) = matches.subcommand_matches("remove") {
        let name = matches.value_of("name").unwrap();
        let version = matches.value_of("version");
        remove(name, Some(version.unwrap())).await;
    } else if let Some(_matches) = matches.subcommand_matches("list") {
        list().await;
    } else {
        eprintln!("No valid subcommand was provided");
    }
}

async fn register(username: &str, password: &str) {
    let client = Client::new();
    let user = User {
        username: username.to_string(),
        password: password.to_string(),
    };

    let response = client
        .post("http://192.168.0.29:5000/register")
        .json(&user)
        .send()
        .await
        .expect("Failed to send request");

    let response_text = response.text().await.expect("Failed to read response text");
    println!("{}", response_text);
}

async fn login(username: &str, password: &str) {
    let client = Client::new();
    let login = User {
        username: username.to_string(),
        password: password.to_string(),
    };

    let response = client
        .post("http://192.168.0.29:5000/login")
        .json(&login)
        .send()
        .await
        .expect("Failed to send request");

    if response.status().is_success() {
        let cookies = response.headers().get(SET_COOKIE);
        match cookies {
            Some(cookie) => {
                println!("Login successful, session: {:?}", cookie);
                let session_id = cookie.to_str().unwrap().to_string();
                fs::write("session_id", &session_id).await.expect("Unable to write file");
                fs::write("username", &username).await.expect("Unable to write file");
            },
            None => {
                println!("No session cookie found");
            },
        }
        } else {
        println!("\x1b[31mLogin failed\x1b[0m");
    }
}

async fn install(name: &str, version: Option<&str>, location: &Path) {
    let client = Client::new();

    let version = match version {
        Some(v) => v.to_string(),
        None => {
            match get_latest_version(name).await {
                Some(v) => v,
                None => {
                    println!("Unable to determine the latest version for {}", name);
                    return;
                }
            }
        }
    };

    println!("Installing {} version {}", name, version);

    let file_path = format!("/tmp/{}-{}.tar.gz", name, version);
    if !Path::new(&file_path).exists() {
        let url = format!("http://192.168.0.29:5000/packages/{}/{}", name, version);
        let response = client.get(&url).send().await.expect("Failed to send request");
        if response.status().is_success() {
            let response_bytes = response.bytes().await.expect("Failed to read response bytes");
            let package_data = response_bytes.clone();
            let mut file = tokio::fs::File::create(&file_path).await;

            match file {
                Ok(mut file) => {
                    file.write_all(&package_data)
                    .await
                    .expect("Unable to write file");
                },
                Err(e) => {
                    eprintln!("Failed to create temp file: {}", e);
                    return;
                }
            }
            println!("Package downloaded successfully and saved to {}", file_path);
            } else {
                println!("\x1b[31mError downloading package {}: {}\x1b[0m", name, response.status());
            }
    } else {
        println!("Using existing file {}-{}.tar.gz", name, version);
    }

    // Extract the downloaded file to "/"
    let extract_path = location;
    
    // Open the file
    let file = File::open(&file_path).expect("Unable to open file");

    // Create the GzDecoder
    let gz_decoder = GzDecoder::new(file);

    // Create the Archive
    let mut archive = Archive::new(gz_decoder);

    // Unpack the archive
    let extract_result = archive.unpack(extract_path);

    match extract_result {
        Ok(_) => println!("Successfully extracted the package."),
        Err(e) => {
            eprintln!("Failed to extract package: {}", e);
            eprintln!("IO error kind: {:?}", e.kind());
            return;
        }
    }

    // Save the names and folders
    let mut names_and_folders: Vec<String> = Vec::new();
    println!("Package extracted successfully to {}", extract_path.display());
    // Save the names and folders
    let mut entries = archive.entries().expect("Failed to read entries");
    while let Some(result) = entries.next() {
        match result {
            Ok(entry) => {
                let path = entry.path().expect("Failed to read path");
                let name = path.to_string_lossy().to_string();
                names_and_folders.push(name.clone());
            }
            Err(e) => {
                println!("Failed to read entry: {}", e);
                return;
            }
        }
    }

    let data_file = "data.json";
    // Read the existing data
    let mut file = match tokio::fs::File::open(data_file).await {
        Ok(file) => {
            println!("File opened successfully");
            file
        },
        Err(_) => {
            println!("Failed to open file, trying to create file");
            match tokio::fs::File::create(data_file).await {
                Ok(file) => {
                    println!("File created successfully");
                    file
                },
                Err(e) => {
                    eprintln!("Failed to create file: {}", e);
                    return;
                }
            }
        }
    };
    let mut contents = Vec::new();
    file.read_to_end(&mut contents).await.unwrap();
    let contents = String::from_utf8_lossy(&contents);

    // Parse the existing data
    let mut data: Vec<serde_json::Value> = serde_json::from_str(&contents).unwrap_or_else(|_| vec![]);

    // Create the new data
    let new_data = serde_json::json!({
        "name": name,
        "version": version,
        "files": names_and_folders,
        "install-loc": extract_path,
    });

    // Append the new data
    if data.is_empty() {
        data = [new_data].to_vec();
    } else {
        data.push(new_data);
    }

    // Convert the data back to a JSON string
    let json_data = serde_json::to_string(&data).expect("Failed to convert data to JSON");

    // Write the JSON string back to the file
    tokio::fs::write(data_file, json_data)
        .await
        .expect("Unable to write data file");

    println!("Data saved to {}", data_file);
    println!("Package installed successfully");

    // Remove the downloaded file
    tokio::fs::remove_file(&file_path).await.expect("Unable to remove file");

    println!("{}-{} Package downloaded successfully and Installed to {}", name, version, file_path);
}

async fn remove(name: &str, version: Option<&str>) {
    let version = match version {
        Some(v) => v.to_string(),
        None => {
            match get_latest_version(name).await {
                Some(v) => v,
                None => {
                    println!("Unable to determine the latest version for {}", name);
                    return;
                }
            }
        }
    };

    println!("Removing {} version {}", name, version);

    let data_file = "/var/lib/birdy/data.json";
    // Read the existing data
    let mut file = tokio::fs::File::open(data_file).await.unwrap();
    let mut contents = Vec::new();
    file.read_to_end(&mut contents).await.unwrap();
    let contents = String::from_utf8_lossy(&contents);

    // Parse the existing data
    let mut data: Vec<serde_json::Value> = serde_json::from_str(&contents).unwrap_or_else(|_| vec![]);

    // Find the package to remove
    let mut index = None;
    for (i, package) in data.iter().enumerate() {
        let package_name = package["name"].as_str().unwrap();
        let package_version = package["version"].as_str().unwrap();
        if package_name == name && package_version == version {
            index = Some(i);
            break;
        }
    }

    match index {
        Some(i) => {
            let package = &data[i];
            let files = package["files"].as_array().unwrap();
            let install_loc = package["install-loc"].as_str().unwrap();
            for file in files {
                let file = file.as_str().unwrap();
                let path = format!("{}{}", install_loc, file);
                tokio::fs::remove_file(&path).await.expect("Unable to remove file");
                println!("Removed {}", path);
            }
            data.remove(i);
        }
        None => {
            println!("Package {} version {} not found", name, version);
            return;
        }
    }

    // Convert the data back to a JSON string
    let json_data = serde_json::to_string(&data).expect("Failed to convert data to JSON");

    // Write the JSON string back to the file
    tokio::fs::write(data_file, json_data)
        .await
        .expect("Unable to write data file");

    println!("Data saved to {}", data_file);
}

async fn list() {
    let data_file = "/var/lib/birdy/data.json";
    // Read the existing data
    let mut file = tokio::fs::File::open(data_file).await.unwrap();
    let mut contents = Vec::new();
    file.read_to_end(&mut contents).await.unwrap();
    let contents = String::from_utf8_lossy(&contents);

    // Parse the existing data
    let data: Vec<serde_json::Value> = serde_json::from_str(&contents).unwrap_or_else(|_| vec![]);

    for package in data {
        let name = package["name"].as_str().unwrap();
        let version = package["version"].as_str().unwrap();
        let install_loc = package["install-loc"].as_str().unwrap();
        println!("{}-{}-{}", name, version, install_loc);
    }
}
    

async fn get_latest_version(name: &str) -> Option<String> {
    let client = Client::new();
    let url = format!("http://192.168.0.29:5000/packages/{}/versions", name);
    let response = client.get(&url).send().await;

    match response {
        Ok(response) => {
            if response.status().is_success() {
                let versions: Vec<String> = response.json().await.expect("Failed to parse JSON");
                if !versions.is_empty() {
                    let latest_version = versions.iter().max().unwrap().to_owned();
                    return Some(latest_version);
                }
            } else {
                println!("\x1b[31mError fetching latest version for {}: {}\x1b[0m", name, response.status());
            }
        }
        Err(e) => {
            println!("\x1b[31mError fetching latest version for {}: {}\x1b[0m", name, e);
        }
    }

    None
}