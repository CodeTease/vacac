use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn target_machine(&mut self) -> Result<TargetMachine, String> {
        Target::initialize_x86(&InitializationConfig::default());
        let triple = TargetMachine::get_default_triple();
        let target = Target::from_triple(&triple).map_err(|e| e.to_string())?;
        let machine = target
            .create_target_machine(
                &triple,
                "",
                "",
                OptimizationLevel::Default,
                RelocMode::PIC,
                CodeModel::Default,
            )
            .ok_or("could not create native LLVM target machine")?;
        self.module.set_triple(&triple);
        self.module
            .set_data_layout(&machine.get_target_data().get_data_layout());
        Ok(machine)
    }

    pub(super) fn write_object(&mut self, path: &Path) -> Result<(), String> {
        let machine = self.target_machine()?;
        machine
            .write_to_file(&self.module, FileType::Object, path)
            .map_err(|e| e.to_string())
    }

    pub(super) fn write_llvm(&mut self, path: &Path) -> Result<(), String> {
        self.target_machine()?;
        self.module.print_to_file(path).map_err(|e| e.to_string())
    }
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
