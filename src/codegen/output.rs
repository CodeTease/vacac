use super::*;
use inkwell::passes::PassBuilderOptions;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BackendSettings {
    machine_level: OptimizationLevel,
    pipeline: Option<&'static str>,
}

impl BackendSettings {
    fn for_level(level: Option<OptLevel>) -> Self {
        match level {
            // Preserve the historical behavior: no IR pipeline, but LLVM's
            // default machine-code optimization level.
            None => Self {
                machine_level: OptimizationLevel::Default,
                pipeline: None,
            },
            Some(OptLevel::O0) => Self {
                machine_level: OptimizationLevel::None,
                pipeline: None,
            },
            Some(OptLevel::O1) => Self {
                machine_level: OptimizationLevel::Less,
                pipeline: Some("default<O1>"),
            },
            Some(OptLevel::O2) => Self {
                machine_level: OptimizationLevel::Default,
                pipeline: Some("default<O2>"),
            },
            Some(OptLevel::O3) => Self {
                machine_level: OptimizationLevel::Aggressive,
                pipeline: Some("default<O3>"),
            },
        }
    }
}

fn target_machine(settings: BackendSettings) -> Result<TargetMachine, Diagnostic> {
    Target::initialize_native(&InitializationConfig::default())
        .map_err(|e| format!("could not initialize the native LLVM target: {e}"))?;
    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple).map_err(|e| e.to_string())?;
    target
        .create_target_machine(
            &triple,
            "",
            "",
            settings.machine_level,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| "could not create native LLVM target machine".into())
}

fn prepare_module(
    module: &Module<'_>,
    machine: &TargetMachine,
    settings: BackendSettings,
) -> Result<(), Diagnostic> {
    module.set_triple(&machine.get_triple());
    module.set_data_layout(&machine.get_target_data().get_data_layout());
    if let Some(pipeline) = settings.pipeline {
        let options = PassBuilderOptions::create();
        options.set_verify_each(true);
        module
            .run_passes(pipeline, machine, options)
            .map_err(|error| format!("LLVM optimization pipeline `{pipeline}` failed: {error}"))?;
    }
    module.verify().map_err(|error| {
        Diagnostic::from(format!("invalid LLVM module after optimization: {error}"))
    })
}

pub(super) fn emit_object(
    program: &Program,
    path: &Path,
    level: Option<OptLevel>,
) -> Result<(), Diagnostic> {
    let context = Context::create();
    let module = Codegen::build_module(&context, program)?;
    let settings = BackendSettings::for_level(level);
    let machine = target_machine(settings)?;
    prepare_module(&module, &machine, settings)?;
    machine
        .write_to_file(&module, FileType::Object, path)
        .map_err(|error| error.to_string().into())
}

pub(super) fn emit_llvm(
    program: &Program,
    path: &Path,
    level: Option<OptLevel>,
) -> Result<(), Diagnostic> {
    let context = Context::create();
    let module = Codegen::build_module(&context, program)?;
    let settings = BackendSettings::for_level(level);
    let machine = target_machine(settings)?;
    prepare_module(&module, &machine, settings)?;
    module
        .print_to_file(path)
        .map_err(|error| error.to_string().into())
}

impl From<ArrayElementType> for ValueType {
    fn from(element: ArrayElementType) -> Self {
        match element {
            ArrayElementType::I64 => Self::I64,
            ArrayElementType::F32 => Self::F32,
            ArrayElementType::F64 => Self::F64,
            ArrayElementType::Bool => Self::Bool,
            ArrayElementType::Str => Self::Str,
            ArrayElementType::Enum(index) => Self::Enum(index),
            ArrayElementType::Struct(index) => Self::Struct(index),
            ArrayElementType::Array(index) => Self::Array(index),
            ArrayElementType::Vector(index) => Self::Vector(index),
            ArrayElementType::Tuple(index) => Self::Tuple(index),
            ArrayElementType::Function(index) => Self::Function(index),
            ArrayElementType::RawPointer(index) => Self::RawPointer(index),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optimization_levels_map_ir_pipelines_and_machine_settings() {
        assert_eq!(
            BackendSettings::for_level(None),
            BackendSettings {
                machine_level: OptimizationLevel::Default,
                pipeline: None,
            }
        );
        for (level, machine_level, pipeline) in [
            (OptLevel::O0, OptimizationLevel::None, None),
            (OptLevel::O1, OptimizationLevel::Less, Some("default<O1>")),
            (
                OptLevel::O2,
                OptimizationLevel::Default,
                Some("default<O2>"),
            ),
            (
                OptLevel::O3,
                OptimizationLevel::Aggressive,
                Some("default<O3>"),
            ),
        ] {
            assert_eq!(
                BackendSettings::for_level(Some(level)),
                BackendSettings {
                    machine_level,
                    pipeline,
                }
            );
        }
    }
}
