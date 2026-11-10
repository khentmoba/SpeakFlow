fn main() {
    let code = speak_flow_lib::cli_entry(std::env::args().collect());
    std::process::exit(code);
}
