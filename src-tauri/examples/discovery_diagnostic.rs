fn main() {
    let result = match which::which("codegraph") {
        Ok(path) => {
            serde_json::json!({"whichFound":true,"whichPath":path,"resolved":project_gateway::resolve_entry(&path).is_ok()})
        }
        Err(error) => serde_json::json!({"whichFound":false,"error":error.to_string()}),
    };
    println!("{result}");
}
