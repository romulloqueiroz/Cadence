fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !(2..=3).contains(&args.len()) {
        eprintln!("usage: plugin-worker <plugin> <shared-memory-name> [plugin-id]");
        std::process::exit(2);
    }
    if let Err(error) =
        plugin_sandbox::worker::run(&args[0], &args[1], args.get(2).map(String::as_str))
    {
        eprintln!("plugin worker: {error}");
        std::process::exit(1);
    }
}
