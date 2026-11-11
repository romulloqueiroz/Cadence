use adapter_clap::ClapModule;
use plugin_api::Plugin;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: clap-inspect <plugin.clap | plugin-binary>");
        return ExitCode::from(2);
    };

    let module = match ClapModule::load(&path) {
        Ok(module) => module,
        Err(error) => {
            eprintln!("{path}: {error}");
            return ExitCode::FAILURE;
        }
    };

    for descriptor in module.descriptors() {
        println!(
            "{}\t{}\t{}\t{}\t{:?}",
            descriptor.id, descriptor.name, descriptor.vendor, descriptor.version, descriptor.kind
        );
    }

    match module.create(None) {
        Ok(plugin) => {
            let capabilities = plugin.capabilities();
            println!(
                "capabilities: in={} out={} latency={} tail={}",
                capabilities.inputs,
                capabilities.outputs,
                capabilities.latency_samples,
                capabilities.tail_samples
            );
            println!("parameters: {}", plugin.parameters().len());
        }
        Err(error) => {
            eprintln!("create: {error}");
            return ExitCode::FAILURE;
        }
    }

    ExitCode::SUCCESS
}
