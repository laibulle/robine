use anyhow::{Context, Result};
use cranelift_codegen::ir::{AbiParam, InstBuilder, Signature, types};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{DataDescription, Linkage, Module, default_libcall_names};
use robine_core::{CoreInstruction, CoreProgram};
use robine_rust_bridge_demo::robine_demo_grapheme_count;
use std::ffi::c_void;
use std::io::Write as _;

pub trait ConsoleSink {
    /// Writes one semantic line to the selected host console.
    ///
    /// # Errors
    ///
    /// Returns the host error reported while writing the line.
    fn write_line(&mut self, text: &str) -> Result<()>;
}

#[derive(Default)]
pub struct CapturedConsole {
    pub lines: Vec<String>,
}

impl ConsoleSink for CapturedConsole {
    fn write_line(&mut self, text: &str) -> Result<()> {
        self.lines.push(text.to_owned());
        Ok(())
    }
}

pub struct StandardConsole;

impl ConsoleSink for StandardConsole {
    fn write_line(&mut self, text: &str) -> Result<()> {
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();
        lock.write_all(text.as_bytes())?;
        lock.write_all(b"\n")?;
        lock.flush()?;
        Ok(())
    }
}

/// Executes the explicit Core as a reference implementation.
///
/// # Errors
///
/// Returns the first error produced by the injected console handler.
pub fn interpret(program: &CoreProgram, console: &mut impl ConsoleSink) -> Result<()> {
    for instruction in &program.instructions {
        match instruction {
            CoreInstruction::ConsoleWrite(text) => console.write_line(text)?,
            CoreInstruction::RustGraphemeCount(text) => {
                // SAFETY: A Robine Text owns a readable UTF-8 buffer for this
                // complete synchronous adapter call.
                let count = unsafe { robine_demo_grapheme_count(text.as_ptr(), text.len()) };
                if count == usize::MAX {
                    anyhow::bail!("la passerelle Rust a rejeté le texte");
                }
            }
        }
    }
    Ok(())
}

struct HostContext<'a> {
    console: &'a mut dyn ConsoleSink,
    error: Option<anyhow::Error>,
}

unsafe extern "C" fn host_console_write(context: *mut c_void, pointer: *const u8, length: usize) {
    if context.is_null() || pointer.is_null() {
        return;
    }
    // SAFETY: The generated function passes the HostContext pointer supplied
    // by run_jit and pointers to immutable JIT data with the declared length.
    let context = unsafe { &mut *context.cast::<HostContext<'_>>() };
    // SAFETY: The pointer and length refer to bytes embedded by this module.
    let bytes = unsafe { std::slice::from_raw_parts(pointer, length) };
    match std::str::from_utf8(bytes)
        .context("le backend a produit un texte UTF-8 invalide")
        .and_then(|text| context.console.write_line(text))
    {
        Ok(()) => {}
        Err(error) => context.error = Some(error),
    }
}

/// Compiles the bootstrap Core with Cranelift and invokes the generated root.
///
/// # Errors
///
/// Returns code generation, finalization or host-console failures with their
/// compilation stage attached.
#[allow(clippy::too_many_lines)]
pub fn run_jit(program: &CoreProgram, console: &mut impl ConsoleSink) -> Result<i32> {
    let mut builder =
        JITBuilder::new(default_libcall_names()).context("création du backend JIT")?;
    builder.symbol("robine_host_console_write", host_console_write as *const u8);
    builder.symbol(
        "robine_demo_grapheme_count",
        robine_demo_grapheme_count as *const u8,
    );
    let mut module = JITModule::new(builder);

    let pointer_type = module.target_config().pointer_type();
    let mut host_signature = module.make_signature();
    host_signature.params.push(AbiParam::new(pointer_type));
    host_signature.params.push(AbiParam::new(pointer_type));
    host_signature.params.push(AbiParam::new(pointer_type));
    let host_id = module
        .declare_function(
            "robine_host_console_write",
            Linkage::Import,
            &host_signature,
        )
        .context("déclaration de la console hôte")?;
    let mut bridge_signature = module.make_signature();
    bridge_signature.params.push(AbiParam::new(pointer_type));
    bridge_signature.params.push(AbiParam::new(pointer_type));
    bridge_signature.returns.push(AbiParam::new(pointer_type));
    let bridge_id = module
        .declare_function(
            "robine_demo_grapheme_count",
            Linkage::Import,
            &bridge_signature,
        )
        .context("déclaration de la passerelle Rust")?;

    let mut entry_signature = Signature::new(CallConv::triple_default(module.isa().triple()));
    entry_signature.params.push(AbiParam::new(pointer_type));
    entry_signature.returns.push(AbiParam::new(types::I32));
    let entry_id = module
        .declare_function("robine_bootstrap_main", Linkage::Export, &entry_signature)
        .context("déclaration de la racine Robine")?;

    let mut context = module.make_context();
    context.func.signature = entry_signature;
    let mut function_builder_context = FunctionBuilderContext::new();
    {
        let mut function_builder =
            FunctionBuilder::new(&mut context.func, &mut function_builder_context);
        let block = function_builder.create_block();
        function_builder.append_block_params_for_function_params(block);
        function_builder.switch_to_block(block);
        function_builder.seal_block(block);
        let host_context = function_builder.block_params(block)[0];
        let host_reference = module.declare_func_in_func(host_id, function_builder.func);
        let bridge_reference = module.declare_func_in_func(bridge_id, function_builder.func);

        for (index, instruction) in program.instructions.iter().enumerate() {
            let text = match instruction {
                CoreInstruction::ConsoleWrite(text) | CoreInstruction::RustGraphemeCount(text) => {
                    text
                }
            };
            let data_name = format!("robine_text_{index}");
            let data_id = module
                .declare_data(&data_name, Linkage::Local, false, false)
                .with_context(|| format!("déclaration de la donnée `{data_name}`"))?;
            let mut data = DataDescription::new();
            data.define(text.as_bytes().to_vec().into_boxed_slice());
            module
                .define_data(data_id, &data)
                .with_context(|| format!("définition de la donnée `{data_name}`"))?;
            let global = module.declare_data_in_func(data_id, function_builder.func);
            let pointer = function_builder.ins().global_value(pointer_type, global);
            let length = function_builder
                .ins()
                .iconst(pointer_type, i64::try_from(text.len())?);
            match instruction {
                CoreInstruction::ConsoleWrite(_) => {
                    function_builder
                        .ins()
                        .call(host_reference, &[host_context, pointer, length]);
                }
                CoreInstruction::RustGraphemeCount(_) => {
                    function_builder
                        .ins()
                        .call(bridge_reference, &[pointer, length]);
                }
            }
        }
        let success = function_builder.ins().iconst(types::I32, 0);
        function_builder.ins().return_(&[success]);
        function_builder.finalize();
    }
    module
        .define_function(entry_id, &mut context)
        .context("génération de la racine Robine")?;
    module.clear_context(&mut context);
    module
        .finalize_definitions()
        .context("finalisation du programme JIT")?;

    let code = module.get_finalized_function(entry_id);
    // SAFETY: Cranelift generated this function from entry_signature above.
    let entry: unsafe extern "C" fn(*mut c_void) -> i32 = unsafe { std::mem::transmute(code) };
    let mut host = HostContext {
        console,
        error: None,
    };
    // SAFETY: The pointer is valid for the duration of the generated call.
    let status = unsafe { entry(std::ptr::from_mut(&mut host).cast::<c_void>()) };
    if let Some(error) = host.error {
        return Err(error).context("écriture de la console Robine");
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program() -> CoreProgram {
        CoreProgram {
            instructions: vec![
                CoreInstruction::RustGraphemeCount("👩🏽‍💻".to_owned()),
                CoreInstruction::ConsoleWrite("un".to_owned()),
                CoreInstruction::ConsoleWrite("deux".to_owned()),
            ],
        }
    }

    #[test]
    fn interpreter_preserves_console_order() {
        let mut console = CapturedConsole::default();
        interpret(&program(), &mut console).expect("interpretation succeeds");
        assert_eq!(console.lines, ["un", "deux"]);
    }

    #[test]
    fn jit_matches_reference_interpreter() {
        let mut reference = CapturedConsole::default();
        interpret(&program(), &mut reference).expect("reference succeeds");
        let mut native = CapturedConsole::default();
        let status = run_jit(&program(), &mut native).expect("JIT succeeds");
        assert_eq!(status, 0);
        assert_eq!(native.lines, reference.lines);
    }
}
