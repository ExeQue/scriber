mod agents;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("agents") {
        return agents::cli(&args[1..]);
    }
    println!("scriber");
    Ok(())
}
