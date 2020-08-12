use clap::{App, Arg};
use crypto::digest::Digest;
use crypto::sha2::Sha256;
use futures::{stream, StreamExt};
use hyper::http::StatusCode;
use reqwest::header;
use std::fs;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufRead;
use std::io::BufReader;
use std::io::LineWriter;
use std::path::Path;
use std::str;
use std::time::Duration;

use tokio;

fn read_lines(path: &str) -> std::io::Result<Vec<String>> {
    let file = File::open(path).expect("Unable to open file.");
    let reader = BufReader::new(file);
    let v: Vec<String> = reader.lines().filter_map(Result::ok).collect();
    Ok(v)
}
fn write_results(output_data: &Vec<String>, output_body: String, output_dir: &str) {
    let unique_url = output_data[0].as_str();
    let start_index = unique_url.find('/').unwrap() + 2;
    let temp_dir = &unique_url[start_index..];
    let end_index = temp_dir.find('/').unwrap();
    let main_dir = output_dir;
    let final_dir = format!("{}/{}", main_dir, &temp_dir[..end_index]);

    let mut hasher = Sha256::new();
    hasher.input_str(unique_url);
    let test_hash = hasher.result_str();
    fs::create_dir_all(&final_dir).unwrap();

    let path = Path::new(&final_dir).join(test_hash);
    let display = path.display();

    let file = match File::create(&path) {
        Err(why) => panic!("couldn't create {}: {}", display, why),
        Ok(file) => file,
    };

    let mut file = LineWriter::new(file);

    for (pos, i) in output_data.iter().enumerate() {
        if pos == 1 {
            file.write_all(b"\n> GET /test.html HTTP/1.1\n")
                .expect("didn't work");
            file.write_all(b"> Host: test.\n").expect("nope");
            file.write_all(b"> User-Agent: Mozilla/5.0 (compatible; fes/0.1; +https://github.com/JohnWoodman/fes)\n\n").expect("nope");
            let status = StatusCode::from_bytes(i.as_bytes()).unwrap();
            let full_status = format!("< {} {}\n", i, status.canonical_reason().unwrap());
            file.write_all(full_status.as_bytes())
                .expect("Coudlnt' write");
        } else if pos == 0 {
            file.write_all(i.as_bytes()).expect("Unable to write data");
            file.write_all(b"\n").expect("Unable to write new line");
        } else {
            file.write_all(b"< ").expect("Unable to write data");
            file.write_all(i.as_bytes()).expect("Unable to write data");
            file.write_all(b"\n").expect("Unable to write new line");
        }
    }

    file.write_all(b"\n").expect("Unable to write new line");
    file.write_all(output_body.as_bytes())
        .expect("Unable to write new line");
    file.flush().expect("Could not flush file");
}
fn main() {
    let matches = App::new("fes")
        .version("1.0")
        .author("John Woodman <john.woodman11@gmail.com>")
        .about("Fast Endpoint Scanner Built In Rust")
        .arg(
            Arg::with_name("paths_file")
                .short("p")
                .long("path")
                .takes_value(true)
                .required(true)
                .help("File with list of endpoints"),
        )
        .arg(
            Arg::with_name("urls_file")
                .short("u")
                .long("urls")
                .takes_value(true)
                .required(true)
                .help("File with list of urls"),
        )
        .arg(
            Arg::with_name("num")
                .short("c")
                .long("concurrency")
                .takes_value(true)
                .help("Set the number of parallel requests (default: 20)"),
        )
        .arg(
            Arg::with_name("output_dir")
                .short("o")
                .long("output")
                .takes_value(true)
                .help("Specify the directory for output (default: fes_out)"),
        )
        .get_matches();

    let urls_file = matches.value_of("urls_file").unwrap();
    let paths_file = matches.value_of("paths_file").unwrap();
    let output_dir = matches.value_of("output_dir").unwrap_or("fes_out");
    let parallel_requests: usize = matches.value_of("num").unwrap_or("20").parse().unwrap();
    let url_string: Vec<String> = read_lines(urls_file).unwrap();
    let urls: Vec<&str> = url_string.iter().map(AsRef::as_ref).collect();
    let paths_string: Vec<String> = read_lines(paths_file).unwrap();
    let paths: Vec<&str> = paths_string.iter().map(AsRef::as_ref).collect();

    get_request(urls, paths, parallel_requests, output_dir);
}
#[tokio::main]
async fn get_request(
    urls: Vec<&str>,
    paths: Vec<&str>,
    parallel_requests: usize,
    output_dir: &str,
) {
    let custom_redirect = reqwest::redirect::Policy::none();
    let mut headers = header::HeaderMap::new();
    headers.insert(
        header::USER_AGENT,
        header::HeaderValue::from_static(
            "Mozilla/5.0 (compatible, fes/0.1; +https://github.com/JohnWoodman/fes)",
        ),
    );
    let client = reqwest::Client::builder()
        .redirect(custom_redirect)
        .default_headers(headers)
        .build()
        .unwrap();

    for path in paths {
        let urls = urls.clone();
        let bodies = stream::iter(urls)
            .map(|url| {
                let client = &client;
                async move {
                    let mut full_url = String::new();
                    full_url.push_str(url);
                    full_url.push_str(path);
                    let resp = client
                        .get(&full_url)
                        .timeout(Duration::from_secs(3))
                        .send()
                        .await;
                    resp
                }
            })
            .buffer_unordered(parallel_requests);

        bodies
            .for_each(|b| async {
                match b {
                    Ok(b) => {
                        //Access all data from response BEFORE accessing bytes, because b variable
                        //gets moved when accessing .bytes() for some reason (probably the .await)
                        let mut vec = Vec::new();
                        let url = b.url().as_str().to_string();
                        let headers = &b.headers();
                        let status = b.status().as_str().to_string();
                        vec.push(url);
                        vec.push(status);
                        for (key, value) in headers.iter() {
                            let pair = format!("{}: {}", key.as_str(), value.to_str().unwrap());
                            vec.push(pair);
                        }
                        let body_test = b.text().await.unwrap();
                        write_results(&vec, body_test.to_string(), output_dir);
                    }
                    Err(e) => println!("Got an error: {}", e),
                }
            })
            .await;
    }
}

/* ----------TODO----------
 * Figure out how to parse HTML for keywords, also save response to file (like meg)
 * For the lightweight version (less diskspace), hash the response and store that instead of the
 * full response. Then check for anomalies based off threshold given (or just sort all hashes,
 * putting the unique ones first.
 */
