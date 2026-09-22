// src/webui/main.rs
// Standalone `webui` binary — thin entry point that delegates to the shared lib.

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let port = get_flag_value(&args, "--port")
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(7979);

    let open_browser = args.iter().any(|a| a == "--open" || a == "-o");

    vivaldi_mod_interceptor::webui::run(port, open_browser);
}

fn get_flag_value(args: &[String], flag: &str) -> Option<String> {
    for (i, arg) in args.iter().enumerate() {
        if arg == flag && i + 1 < args.len() {
            return Some(args[i + 1].clone());
        }
        if let Some(stripped) = arg.strip_prefix(&format!("{}=", flag)) {
            return Some(stripped.to_string());
        }
    }
    None
}
