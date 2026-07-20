use anyhow::{Context, Result, bail, ensure};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{
    AbiParam, BlockArg, InstBuilder, Signature, Type as CraneliftType, Value, types,
};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{DataDescription, FuncId, Linkage, Module, default_libcall_names};
use robine_core::{
    CoreBinaryOp, CoreExpr, CoreExprKind, CoreFunction, CoreProgram, CoreStatement, Type,
};
use robine_rust_bridge_demo::robine_demo_grapheme_count;
use std::collections::BTreeMap;
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

#[derive(Clone, Debug, Eq, PartialEq)]
enum RuntimeValue {
    Unit,
    Bool(bool),
    Int(i64),
    Text(String),
    Console,
}

struct Interpreter<'program, 'console> {
    program: &'program CoreProgram,
    console: &'console mut dyn ConsoleSink,
}

impl Interpreter<'_, '_> {
    fn call(&mut self, function_name: &str, args: Vec<RuntimeValue>) -> Result<RuntimeValue> {
        let function = self
            .program
            .functions
            .iter()
            .find(|function| function.name == function_name)
            .cloned()
            .with_context(|| format!("fonction Core `{function_name}` absente"))?;
        ensure!(
            function.params.len() == args.len(),
            "`{function_name}` attend {} argument(s), reçu {}",
            function.params.len(),
            args.len()
        );
        let mut environment = function
            .params
            .iter()
            .map(|(name, _)| name.clone())
            .zip(args)
            .collect::<BTreeMap<_, _>>();
        let mut result = RuntimeValue::Unit;
        for statement in &function.body {
            match statement {
                CoreStatement::Let { name, value } => {
                    let value = self.evaluate(value, &environment)?;
                    environment.insert(name.clone(), value);
                    result = RuntimeValue::Unit;
                }
                CoreStatement::Expr { value, terminated } => {
                    let value = self.evaluate(value, &environment)?;
                    result = if *terminated {
                        RuntimeValue::Unit
                    } else {
                        value
                    };
                }
            }
        }
        ensure!(
            runtime_type(&result) == function.return_type,
            "`{function_name}` a produit `{}`, Core attendu `{}`",
            runtime_type(&result),
            function.return_type
        );
        Ok(result)
    }

    fn evaluate(
        &mut self,
        expression: &CoreExpr,
        environment: &BTreeMap<String, RuntimeValue>,
    ) -> Result<RuntimeValue> {
        match &expression.kind {
            CoreExprKind::Unit => Ok(RuntimeValue::Unit),
            CoreExprKind::Bool(value) => Ok(RuntimeValue::Bool(*value)),
            CoreExprKind::Int(value) => Ok(RuntimeValue::Int(*value)),
            CoreExprKind::Text(value) => Ok(RuntimeValue::Text(value.clone())),
            CoreExprKind::Local(name) => environment
                .get(name)
                .cloned()
                .with_context(|| format!("binding Core `{name}` absent")),
            CoreExprKind::If {
                condition,
                consequence,
                alternative,
            } => match self.evaluate(condition, environment)? {
                RuntimeValue::Bool(true) => self.evaluate(consequence, environment),
                RuntimeValue::Bool(false) => self.evaluate(alternative, environment),
                other => bail!(
                    "condition Core `{}` au lieu de `Bool`",
                    runtime_type(&other)
                ),
            },
            CoreExprKind::Binary {
                operator,
                left,
                right,
            } => {
                let RuntimeValue::Int(left) = self.evaluate(left, environment)? else {
                    bail!("opérande Core gauche non entière");
                };
                let RuntimeValue::Int(right) = self.evaluate(right, environment)? else {
                    bail!("opérande Core droite non entière");
                };
                Ok(match operator {
                    CoreBinaryOp::Add => RuntimeValue::Int(left.wrapping_add(right)),
                    CoreBinaryOp::Subtract => RuntimeValue::Int(left.wrapping_sub(right)),
                    CoreBinaryOp::Multiply => RuntimeValue::Int(left.wrapping_mul(right)),
                    CoreBinaryOp::Equal => RuntimeValue::Bool(left == right),
                    CoreBinaryOp::LessThan => RuntimeValue::Bool(left < right),
                    CoreBinaryOp::LessThanOrEqual => RuntimeValue::Bool(left <= right),
                })
            }
            CoreExprKind::Call { function, args } => {
                let mut values = Vec::with_capacity(args.len());
                for argument in args {
                    values.push(self.evaluate(argument, environment)?);
                }
                self.call(function, values)
            }
            CoreExprKind::ConsoleWrite { receiver, text } => {
                ensure!(
                    self.evaluate(receiver, environment)? == RuntimeValue::Console,
                    "`Console.write_line` a reçu une capacité invalide"
                );
                let RuntimeValue::Text(text) = self.evaluate(text, environment)? else {
                    bail!("`Console.write_line` a reçu une valeur Core non textuelle");
                };
                self.console.write_line(&text)?;
                Ok(RuntimeValue::Unit)
            }
            CoreExprKind::RustGraphemeCount { text } => {
                let RuntimeValue::Text(text) = self.evaluate(text, environment)? else {
                    bail!("la passerelle Rust a reçu une valeur Core non textuelle");
                };
                // SAFETY: A Robine Text owns a readable UTF-8 buffer for this
                // complete synchronous adapter call.
                let count = unsafe { robine_demo_grapheme_count(text.as_ptr(), text.len()) };
                ensure!(count != usize::MAX, "la passerelle Rust a rejeté le texte");
                Ok(RuntimeValue::Int(i64::try_from(count)?))
            }
        }
    }
}

fn runtime_type(value: &RuntimeValue) -> Type {
    match value {
        RuntimeValue::Unit => Type::Unit,
        RuntimeValue::Bool(_) => Type::Bool,
        RuntimeValue::Int(_) => Type::Int,
        RuntimeValue::Text(_) => Type::Text,
        RuntimeValue::Console => Type::Console,
    }
}

/// Executes the explicit Core as a reference implementation.
///
/// # Errors
///
/// Returns the first invalid Core invariant or host error.
pub fn interpret(program: &CoreProgram, console: &mut impl ConsoleSink) -> Result<()> {
    let entry = program
        .functions
        .iter()
        .find(|function| function.name == program.entry)
        .with_context(|| format!("racine Core `{}` absente", program.entry))?;
    let args = entry
        .params
        .iter()
        .map(|(_, type_name)| match type_name {
            Type::Console => Ok(RuntimeValue::Console),
            other => bail!("paramètre racine Core `{other}` non fourni par l’hôte"),
        })
        .collect::<Result<Vec<_>>>()?;
    let result = Interpreter { program, console }.call(&program.entry, args)?;
    ensure!(
        result == RuntimeValue::Unit,
        "la racine Core doit retourner `Unit`"
    );
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

#[derive(Clone)]
struct DeclaredFunction {
    id: FuncId,
    params: Vec<Type>,
    return_type: Type,
}

#[derive(Clone, Copy)]
enum JitValue {
    Unit,
    Scalar(Value),
    Text { pointer: Value, length: Value },
}

impl JitValue {
    fn flattened(self) -> Vec<Value> {
        match self {
            Self::Unit => Vec::new(),
            Self::Scalar(value) => vec![value],
            Self::Text { pointer, length } => vec![pointer, length],
        }
    }
}

struct ImportedFunctions {
    console_write: FuncId,
    grapheme_count: FuncId,
}

struct FunctionCompiler<'borrow, 'context> {
    module: &'borrow mut JITModule,
    builder: &'borrow mut FunctionBuilder<'context>,
    functions: &'borrow BTreeMap<String, DeclaredFunction>,
    imports: &'borrow ImportedFunctions,
    pointer_type: CraneliftType,
    data_ordinal: &'borrow mut usize,
    locals: BTreeMap<String, JitValue>,
}

impl FunctionCompiler<'_, '_> {
    fn expression(&mut self, expression: &CoreExpr) -> Result<JitValue> {
        match &expression.kind {
            CoreExprKind::Unit => Ok(JitValue::Unit),
            CoreExprKind::Bool(value) => Ok(JitValue::Scalar(
                self.builder.ins().iconst(types::I8, i64::from(*value)),
            )),
            CoreExprKind::Int(value) => Ok(JitValue::Scalar(
                self.builder.ins().iconst(types::I64, *value),
            )),
            CoreExprKind::Text(value) => self.text_literal(value),
            CoreExprKind::Local(name) => self
                .locals
                .get(name)
                .copied()
                .with_context(|| format!("binding JIT `{name}` absent")),
            CoreExprKind::If {
                condition,
                consequence,
                alternative,
            } => self.if_expression(condition, consequence, alternative, &expression.type_name),
            CoreExprKind::Binary {
                operator,
                left,
                right,
            } => {
                let left = scalar(self.expression(left)?, "Int")?;
                let right = scalar(self.expression(right)?, "Int")?;
                let value = match operator {
                    CoreBinaryOp::Add => self.builder.ins().iadd(left, right),
                    CoreBinaryOp::Subtract => self.builder.ins().isub(left, right),
                    CoreBinaryOp::Multiply => self.builder.ins().imul(left, right),
                    CoreBinaryOp::Equal => self.builder.ins().icmp(IntCC::Equal, left, right),
                    CoreBinaryOp::LessThan => {
                        self.builder.ins().icmp(IntCC::SignedLessThan, left, right)
                    }
                    CoreBinaryOp::LessThanOrEqual => {
                        self.builder
                            .ins()
                            .icmp(IntCC::SignedLessThanOrEqual, left, right)
                    }
                };
                Ok(JitValue::Scalar(value))
            }
            CoreExprKind::Call { function, args } => self.call(function, args),
            CoreExprKind::ConsoleWrite { receiver, text } => {
                let context = scalar(self.expression(receiver)?, "Console")?;
                let (pointer, length) = text_value(self.expression(text)?)?;
                let reference = self
                    .module
                    .declare_func_in_func(self.imports.console_write, self.builder.func);
                self.builder
                    .ins()
                    .call(reference, &[context, pointer, length]);
                Ok(JitValue::Unit)
            }
            CoreExprKind::RustGraphemeCount { text } => {
                let (pointer, length) = text_value(self.expression(text)?)?;
                let reference = self
                    .module
                    .declare_func_in_func(self.imports.grapheme_count, self.builder.func);
                let call = self.builder.ins().call(reference, &[pointer, length]);
                let raw = self.builder.inst_results(call)[0];
                let value = if self.pointer_type == types::I64 {
                    raw
                } else {
                    self.builder.ins().uextend(types::I64, raw)
                };
                Ok(JitValue::Scalar(value))
            }
        }
    }

    fn call(&mut self, function_name: &str, args: &[CoreExpr]) -> Result<JitValue> {
        let declaration = self
            .functions
            .get(function_name)
            .cloned()
            .with_context(|| format!("fonction JIT `{function_name}` absente"))?;
        ensure!(
            declaration.params.len() == args.len(),
            "arité Core invalide pour `{function_name}`"
        );
        let mut flattened = Vec::new();
        for argument in args {
            flattened.extend(self.expression(argument)?.flattened());
        }
        let reference = self
            .module
            .declare_func_in_func(declaration.id, self.builder.func);
        let call = self.builder.ins().call(reference, &flattened);
        let results = self.builder.inst_results(call);
        value_from_slice(&declaration.return_type, results)
    }

    fn if_expression(
        &mut self,
        condition: &CoreExpr,
        consequence: &CoreExpr,
        alternative: &CoreExpr,
        result_type: &Type,
    ) -> Result<JitValue> {
        let condition = scalar(self.expression(condition)?, "Bool")?;
        let consequence_block = self.builder.create_block();
        let alternative_block = self.builder.create_block();
        let merge_block = self.builder.create_block();
        append_block_params(self.builder, merge_block, result_type, self.pointer_type)?;
        self.builder
            .ins()
            .brif(condition, consequence_block, &[], alternative_block, &[]);

        self.builder.switch_to_block(consequence_block);
        let value = self.expression(consequence)?;
        let args = value
            .flattened()
            .into_iter()
            .map(BlockArg::from)
            .collect::<Vec<_>>();
        self.builder.ins().jump(merge_block, &args);
        self.builder.seal_block(consequence_block);

        self.builder.switch_to_block(alternative_block);
        let value = self.expression(alternative)?;
        let args = value
            .flattened()
            .into_iter()
            .map(BlockArg::from)
            .collect::<Vec<_>>();
        self.builder.ins().jump(merge_block, &args);
        self.builder.seal_block(alternative_block);

        self.builder.seal_block(merge_block);
        self.builder.switch_to_block(merge_block);
        value_from_slice(result_type, self.builder.block_params(merge_block))
    }

    fn text_literal(&mut self, text: &str) -> Result<JitValue> {
        let data_name = format!("robine_text_{}", *self.data_ordinal);
        *self.data_ordinal += 1;
        let data_id = self
            .module
            .declare_data(&data_name, Linkage::Local, false, false)
            .with_context(|| format!("déclaration de la donnée `{data_name}`"))?;
        let mut data = DataDescription::new();
        let bytes = if text.is_empty() {
            vec![0]
        } else {
            text.as_bytes().to_vec()
        };
        data.define(bytes.into_boxed_slice());
        self.module
            .define_data(data_id, &data)
            .with_context(|| format!("définition de la donnée `{data_name}`"))?;
        let global = self.module.declare_data_in_func(data_id, self.builder.func);
        let pointer = self.builder.ins().global_value(self.pointer_type, global);
        let length = self
            .builder
            .ins()
            .iconst(self.pointer_type, i64::try_from(text.len())?);
        Ok(JitValue::Text { pointer, length })
    }
}

fn scalar(value: JitValue, expected: &str) -> Result<Value> {
    let JitValue::Scalar(value) = value else {
        bail!("valeur JIT `{expected}` non scalaire");
    };
    Ok(value)
}

fn text_value(value: JitValue) -> Result<(Value, Value)> {
    let JitValue::Text { pointer, length } = value else {
        bail!("valeur JIT non textuelle");
    };
    Ok((pointer, length))
}

fn value_from_slice(type_name: &Type, values: &[Value]) -> Result<JitValue> {
    match type_name {
        Type::Unit => {
            ensure!(values.is_empty(), "`Unit` ne doit produire aucune valeur");
            Ok(JitValue::Unit)
        }
        Type::Text => {
            let [pointer, length] = values else {
                bail!("`Text` doit produire deux valeurs ABI");
            };
            Ok(JitValue::Text {
                pointer: *pointer,
                length: *length,
            })
        }
        Type::Bool | Type::Int | Type::Console => {
            let [value] = values else {
                bail!("`{type_name}` doit produire une valeur ABI");
            };
            Ok(JitValue::Scalar(*value))
        }
        Type::Unknown(name) => bail!("type Core inconnu `{name}`"),
    }
}

fn append_block_params(
    builder: &mut FunctionBuilder<'_>,
    block: cranelift_codegen::ir::Block,
    type_name: &Type,
    pointer_type: CraneliftType,
) -> Result<()> {
    for abi_type in flattened_types(type_name, pointer_type)? {
        builder.append_block_param(block, abi_type);
    }
    Ok(())
}

fn flattened_types(type_name: &Type, pointer_type: CraneliftType) -> Result<Vec<CraneliftType>> {
    match type_name {
        Type::Unit => Ok(Vec::new()),
        Type::Bool => Ok(vec![types::I8]),
        Type::Int => Ok(vec![types::I64]),
        Type::Text => Ok(vec![pointer_type, pointer_type]),
        Type::Console => Ok(vec![pointer_type]),
        Type::Unknown(name) => bail!("type Core inconnu `{name}`"),
    }
}

fn core_signature(module: &JITModule, function: &CoreFunction) -> Result<Signature> {
    let pointer_type = module.target_config().pointer_type();
    let mut signature = module.make_signature();
    for (_, type_name) in &function.params {
        signature.params.extend(
            flattened_types(type_name, pointer_type)?
                .into_iter()
                .map(AbiParam::new),
        );
    }
    signature.returns.extend(
        flattened_types(&function.return_type, pointer_type)?
            .into_iter()
            .map(AbiParam::new),
    );
    Ok(signature)
}

fn declare_imports(
    module: &mut JITModule,
    pointer_type: CraneliftType,
) -> Result<ImportedFunctions> {
    let mut console_signature = module.make_signature();
    console_signature.params.extend(
        [pointer_type, pointer_type, pointer_type]
            .into_iter()
            .map(AbiParam::new),
    );
    let console_write = module
        .declare_function(
            "robine_host_console_write",
            Linkage::Import,
            &console_signature,
        )
        .context("déclaration de la console hôte")?;

    let mut bridge_signature = module.make_signature();
    bridge_signature
        .params
        .extend([pointer_type, pointer_type].into_iter().map(AbiParam::new));
    bridge_signature.returns.push(AbiParam::new(pointer_type));
    let grapheme_count = module
        .declare_function(
            "robine_demo_grapheme_count",
            Linkage::Import,
            &bridge_signature,
        )
        .context("déclaration de la passerelle Rust")?;
    Ok(ImportedFunctions {
        console_write,
        grapheme_count,
    })
}

fn declare_core_functions(
    module: &mut JITModule,
    program: &CoreProgram,
) -> Result<BTreeMap<String, DeclaredFunction>> {
    let mut declarations = BTreeMap::new();
    for (index, function) in program.functions.iter().enumerate() {
        let signature = core_signature(module, function)?;
        let id = module
            .declare_function(
                &format!("robine_function_{index}"),
                Linkage::Local,
                &signature,
            )
            .with_context(|| format!("déclaration de `{}`", function.name))?;
        let previous = declarations.insert(
            function.name.clone(),
            DeclaredFunction {
                id,
                params: function
                    .params
                    .iter()
                    .map(|(_, type_name)| type_name.clone())
                    .collect(),
                return_type: function.return_type.clone(),
            },
        );
        ensure!(
            previous.is_none(),
            "fonction Core `{}` déclarée deux fois",
            function.name
        );
    }
    Ok(declarations)
}

fn define_core_function(
    module: &mut JITModule,
    function: &CoreFunction,
    declaration: &DeclaredFunction,
    functions: &BTreeMap<String, DeclaredFunction>,
    imports: &ImportedFunctions,
    data_ordinal: &mut usize,
) -> Result<()> {
    let mut context = module.make_context();
    context.func.signature = core_signature(module, function)?;
    let mut builder_context = FunctionBuilderContext::new();
    {
        let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);

        let mut parameter_ordinal = 0;
        let mut locals = BTreeMap::new();
        for (name, type_name) in &function.params {
            let width = flattened_types(type_name, module.target_config().pointer_type())?.len();
            let end = parameter_ordinal + width;
            let values = &builder.block_params(entry)[parameter_ordinal..end];
            locals.insert(name.clone(), value_from_slice(type_name, values)?);
            parameter_ordinal = end;
        }

        {
            let pointer_type = module.target_config().pointer_type();
            let mut compiler = FunctionCompiler {
                module,
                builder: &mut builder,
                functions,
                imports,
                pointer_type,
                data_ordinal,
                locals,
            };
            let mut result = JitValue::Unit;
            for statement in &function.body {
                match statement {
                    CoreStatement::Let { name, value } => {
                        let value = compiler.expression(value)?;
                        compiler.locals.insert(name.clone(), value);
                        result = JitValue::Unit;
                    }
                    CoreStatement::Expr { value, terminated } => {
                        let value = compiler.expression(value)?;
                        result = if *terminated { JitValue::Unit } else { value };
                    }
                }
            }
            compiler.builder.ins().return_(&result.flattened());
        }
        builder.finalize();
    }
    module
        .define_function(declaration.id, &mut context)
        .with_context(|| format!("génération de `{}`", function.name))?;
    module.clear_context(&mut context);
    Ok(())
}

fn define_entry_wrapper(module: &mut JITModule, entry: &DeclaredFunction) -> Result<FuncId> {
    let pointer_type = module.target_config().pointer_type();
    let mut signature = Signature::new(CallConv::triple_default(module.isa().triple()));
    signature.params.push(AbiParam::new(pointer_type));
    signature.returns.push(AbiParam::new(types::I32));
    let wrapper = module
        .declare_function("robine_bootstrap_main", Linkage::Export, &signature)
        .context("déclaration de la racine Robine")?;
    let mut context = module.make_context();
    context.func.signature = signature;
    let mut builder_context = FunctionBuilderContext::new();
    {
        let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
        let block = builder.create_block();
        builder.append_block_params_for_function_params(block);
        builder.switch_to_block(block);
        builder.seal_block(block);
        let host_context = builder.block_params(block)[0];
        let mut args = Vec::new();
        for type_name in &entry.params {
            match type_name {
                Type::Console => args.push(host_context),
                other => bail!("paramètre racine JIT `{other}` non fourni par l’hôte"),
            }
        }
        let reference = module.declare_func_in_func(entry.id, builder.func);
        builder.ins().call(reference, &args);
        let success = builder.ins().iconst(types::I32, 0);
        builder.ins().return_(&[success]);
        builder.finalize();
    }
    module
        .define_function(wrapper, &mut context)
        .context("génération de la racine Robine")?;
    module.clear_context(&mut context);
    Ok(wrapper)
}

/// Compiles the structured bootstrap Core with Cranelift and invokes its root.
///
/// # Errors
///
/// Returns invalid Core, code generation, finalization or host failures with
/// their compilation stage attached.
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
    let imports = declare_imports(&mut module, pointer_type)?;
    let functions = declare_core_functions(&mut module, program)?;
    let mut data_ordinal = 0;
    for function in &program.functions {
        let declaration = functions
            .get(&function.name)
            .with_context(|| format!("déclaration de `{}` absente", function.name))?;
        define_core_function(
            &mut module,
            function,
            declaration,
            &functions,
            &imports,
            &mut data_ordinal,
        )?;
    }
    let entry = functions
        .get(&program.entry)
        .with_context(|| format!("racine Core `{}` absente", program.entry))?;
    ensure!(
        entry.return_type == Type::Unit,
        "la racine JIT doit retourner `Unit`"
    );
    let wrapper = define_entry_wrapper(&mut module, entry)?;
    module
        .finalize_definitions()
        .context("finalisation du programme JIT")?;

    let code = module.get_finalized_function(wrapper);
    // SAFETY: Cranelift generated this function from the wrapper signature.
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
    use robine_core::{analyze, lower_entry};

    const PROGRAM: &str = r#"module flow

fn fibonacci(n: Int) -> Int {
    if n <= 1 { n } else { fibonacci(n - 1) + fibonacci(n - 2) }
}

fn emit(console: Console, text: Text) -> Text ! { Console.Write } {
    console.write_line(text);
    text
}

fn second(first: Text, second: Text) -> Text {
    second
}

fn main(console: Console) -> Unit ! { Console.Write } {
    let computed = fibonacci(10);
    let picked = if computed == 55 { "right" } else { "wrong" };
    let result = second(emit(console, "one"), emit(console, picked));
    console.write_line(result)
}
"#;

    const OVERFLOW: &str = r#"module overflow

fn main(console: Console) -> Unit ! { Console.Write } {
    let wrapped = 9223372036854775807 + 1;
    console.write_line(if wrapped < 0 { "wrapped" } else { "broken" })
}
"#;

    fn compile(source: &str, entry: &str) -> CoreProgram {
        let analysis = analyze(source);
        assert_eq!(analysis.diagnostics, Vec::new());
        lower_entry(&analysis, entry).expect("program lowers")
    }

    fn program() -> CoreProgram {
        compile(PROGRAM, "flow.main")
    }

    #[test]
    fn interpreter_preserves_call_and_argument_order() {
        let mut console = CapturedConsole::default();
        interpret(&program(), &mut console).expect("interpretation succeeds");
        assert_eq!(console.lines, ["one", "right", "right"]);
    }

    #[test]
    fn jit_matches_reference_interpreter() {
        let program = program();
        let mut reference = CapturedConsole::default();
        interpret(&program, &mut reference).expect("reference succeeds");
        let mut native = CapturedConsole::default();
        let status = run_jit(&program, &mut native).expect("JIT succeeds");
        assert_eq!(status, 0);
        assert_eq!(native.lines, reference.lines);
    }

    #[test]
    fn jit_and_interpreter_share_wrapping_integer_semantics() {
        let program = compile(OVERFLOW, "overflow.main");
        let mut reference = CapturedConsole::default();
        interpret(&program, &mut reference).expect("reference succeeds");
        let mut native = CapturedConsole::default();
        run_jit(&program, &mut native).expect("JIT succeeds");
        assert_eq!(reference.lines, ["wrapped"]);
        assert_eq!(native.lines, reference.lines);
    }
}
