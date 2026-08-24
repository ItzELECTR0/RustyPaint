fn main() {
    let src = include_str!("../src/gpu/shaders/viewport.wgsl");
    let module = match naga::front::wgsl::parse_str(src) {
        Ok(module) => module,
        Err(e) => {
            eprintln!("{}", e.emit_to_string(src));
            std::process::exit(1);
        }
    };
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    let info = match validator.validate(&module) {
        Ok(info) => info,
        Err(e) => {
            eprintln!("{}", e.emit_to_string(src));
            std::process::exit(1);
        }
    };
    let _ = info;

    let uniforms = module
        .types
        .iter()
        .find(|(_, t)| t.name.as_deref() == Some("Uniforms"))
        .expect("the shader has a Uniforms struct");
    let layouter = {
        let mut l = naga::proc::Layouter::default();
        l.update(module.to_ctx()).expect("layout");
        l
    };
    let size = layouter[uniforms.0].size;
    println!("viewport.wgsl is valid; Uniforms is {size} bytes");
    if size as usize != UNIFORM_BYTES {
        eprintln!("but the Rust struct is {UNIFORM_BYTES}");
        std::process::exit(1);
    }
}

const UNIFORM_BYTES: usize = 464;
