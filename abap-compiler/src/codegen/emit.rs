//! Hilfsfunktionen für LLVM-Aufrufe, Literale und Blöcke.

use super::*;

impl<'ctx> Codegen<'ctx> {
    // --- Hilfsfunktionen ---------------------------------------------------

    pub(super) fn call_on(
        &self,
        b: &Builder<'ctx>,
        f: FunctionValue<'ctx>,
        args: &[BasicMetadataValueEnum<'ctx>],
    ) -> CResult<Option<BasicValueEnum<'ctx>>> {
        let c = llvm(b.build_call(f, args, ""))?;
        Ok(c.try_as_basic_value().basic())
    }

    pub(super) fn call(
        &self,
        f: FunctionValue<'ctx>,
        args: &[BasicMetadataValueEnum<'ctx>],
    ) -> CResult<Option<BasicValueEnum<'ctx>>> {
        self.call_on(&self.builder, f, args)
    }

    pub(super) fn call_void(
        &self,
        f: FunctionValue<'ctx>,
        args: &[BasicMetadataValueEnum<'ctx>],
    ) -> CResult<()> {
        self.call(f, args)?;
        Ok(())
    }

    pub(super) fn call_int(
        &self,
        f: FunctionValue<'ctx>,
        args: &[BasicMetadataValueEnum<'ctx>],
    ) -> CResult<IntValue<'ctx>> {
        match self.call(f, args)? {
            Some(v) => Ok(v.into_int_value()),
            None => err(0, "Runtime function without return value"),
        }
    }

    pub(super) fn call_ptr(
        &self,
        f: FunctionValue<'ctx>,
        args: &[BasicMetadataValueEnum<'ctx>],
    ) -> CResult<PointerValue<'ctx>> {
        match self.call(f, args)? {
            Some(v) => Ok(v.into_pointer_value()),
            None => err(0, "Runtime function without return value"),
        }
    }

    pub(super) fn len(&self, n: usize) -> IntValue<'ctx> {
        self.i64t.const_int(n as u64, false)
    }

    /// Zeiger auf ein konstantes UTF-16-Literal im Datensegment (eine Einheit je Zeichen).
    pub(super) fn lit(&self, s: &str) -> PointerValue<'ctx> {
        let i16t = self.ctx.i16_type();
        let vals: Vec<IntValue<'ctx>> = s
            .encode_utf16()
            .map(|u| i16t.const_int(u as u64, false))
            .collect();
        let arr = i16t.const_array(&vals);
        let g = self.module.add_global(arr.get_type(), None, "lit");
        g.set_initializer(&arr);
        g.set_constant(true);
        g.as_pointer_value()
    }

    /// Feld aus `n` Zeichen (je zwei Byte).
    pub(super) fn char_array(&self, n: usize) -> ArrayType<'ctx> {
        self.ctx.i16_type().array_type(n as u32)
    }

    /// Konstantes Feld der Länge `n` Zeichen: `s` vorne, mit Leerzeichen aufgefüllt, ggf. gekürzt.
    pub(super) fn const_chars(&self, s: &str, n: usize) -> ArrayValue<'ctx> {
        let i16t = self.ctx.i16_type();
        let units: Vec<u16> = s.encode_utf16().collect();
        let vals: Vec<IntValue<'ctx>> = (0..n)
            .map(|i| i16t.const_int(units.get(i).copied().unwrap_or(0x20) as u64, false))
            .collect();
        i16t.const_array(&vals)
    }

    /// Springt von der aktuellen Position zu `target`, falls der Block noch offen ist.
    pub(super) fn br_if_open(&mut self, target: BasicBlock<'ctx>) -> CResult<()> {
        if self.current_open() {
            llvm(self.builder.build_unconditional_branch(target))?;
        }
        Ok(())
    }

    /// Fehler einer Teilauswertung (Bedingung, Vergleich) merken und mit `fallback` weiter übersetzen. Ohne das
    /// blieben Blöcke des Kontrollflusses (etwa `endif`) unabgeschlossen, und der LLVM-Prüfer meldete einen
    /// zweiten, irreführenden Fehler.
    pub(super) fn defer<T>(&mut self, result: CResult<T>, fallback: T) -> T {
        match result {
            Ok(value) => value,
            Err(e) => {
                self.errors.push(e);
                fallback
            }
        }
    }

    pub(super) fn current_open(&self) -> bool {
        self.builder
            .get_insert_block()
            .map(|b| b.get_terminator().is_none())
            .unwrap_or(false)
    }

    pub(super) fn new_block(&self, name: &str) -> CResult<BasicBlock<'ctx>> {
        let f = self
            .body
            .and_then(|b| b.get_parent())
            .ok_or_else(|| CompileError {
                line: 0,
                message: "no function context".into(),
            })?;
        Ok(self.ctx.append_basic_block(f, name))
    }

    /// Neuer Block für Code nach einem unbedingten Sprung (unerreichbar, aber gültig).
    pub(super) fn open_dead_block(&mut self) -> CResult<()> {
        let bb = self.new_block("after")?;
        self.builder.position_at_end(bb);
        Ok(())
    }
}
