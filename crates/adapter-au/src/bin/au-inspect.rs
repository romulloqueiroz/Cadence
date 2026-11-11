use adapter_au::{list_components, AuPlugin};
use plugin_api::Plugin;
use std::process::ExitCode;

fn main() -> ExitCode {
    let query = std::env::args().nth(1);
    match query {
        Some(query) => match AuPlugin::load(&query) {
            Ok(plugin) => {
                let capabilities = plugin.capabilities();
                println!(
                    "{}: in={} out={} latency={} tail={} parameters={}",
                    plugin.descriptor().name,
                    capabilities.inputs,
                    capabilities.outputs,
                    capabilities.latency_samples,
                    capabilities.tail_samples,
                    plugin.parameters().len()
                );
                for parameter in plugin.parameters().iter().take(20) {
                    println!(
                        "  [{}] {} {}..{} = {}",
                        parameter.id,
                        parameter.name,
                        parameter.min,
                        parameter.max,
                        parameter.default
                    );
                }
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{query}: {error}");
                ExitCode::FAILURE
            }
        },
        None => {
            for component in list_components() {
                println!(
                    "{}\t{:08X}\t{:08X}\t{:08X}",
                    component.name,
                    component.component_type,
                    component.sub_type,
                    component.manufacturer
                );
            }
            ExitCode::SUCCESS
        }
    }
}
