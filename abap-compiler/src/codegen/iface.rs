//! Interface-Referenzen (FR-001): Methodentabellen je Klasse und Aufruf über die Tabelle.
//!
//! Layout:
//! - Jedes Interface des Programms bekommt einen festen Offset: Summe der Methodenzahlen der
//!   vorher definierten Interfaces. Ein Slot ist `Offset(Interface) + Index(Methode)`, der Index
//!   ist die Position im sortierten Namen der Interface-Liste (`InterfaceDef::methods`).
//! - Die Tabelle einer Klasse ist eine konstante Tabelle von Funktionszeigern je Slot. Nicht belegte
//!   Slots sind null. Die Tabelle einer Unterklasse beginnt als Kopie der Tabelle der Oberklasse, die
//!   eigenen Implementierungen überschreiben die Einträge (Festlegung in `doc/sprache.md`, Abschnitt 5d).
//! - Ein Objekt trägt 8 Byte vor dem Objektzeiger einen Zeiger auf die Tabelle seiner Klasse. Der
//!   Objektzeiger (und damit auch eine Interface-Referenz) zeigt auf die Attribute, wie bisher.
//! - Aufruf über eine Interface-Referenz: Tabelle aus dem Kopf laden, Slot lesen, indirekt aufrufen.
//!   Es gibt keinen Namensvergleich zur Laufzeit.
//!
//! Die Statik sorgt dafür, dass ein Slot auf einer erreichbaren Referenz belegt ist: Zuweisungen und
//! `CREATE OBJECT ... TYPE` prüfen die Implementierung, und nicht implementierte Methoden sind ein
//! Übersetzungsfehler (`check_all_implemented`).

use super::*;
use crate::lower::{InterfaceDef, InterfaceMethod, InterfaceRef};

/// Größe des Kopfes vor dem Objektzeiger in Bytes (Zeiger auf die Methodentabelle der Klasse)
pub(super) const OBJECT_HEADER: usize = 8;

/// Fügt ein Interface und seine Unter-Interfaces (über `INTERFACES` im Interface) hinzu.
fn add_iface_closure(ifaces: &[InterfaceDef], i: usize, out: &mut Vec<usize>) {
    if out.contains(&i) {
        return;
    }
    out.push(i);
    for &parent in &ifaces[i].parents {
        add_iface_closure(ifaces, parent, out);
    }
}

/// Die Signatur einer Implementierung muss der Interface-Methode entsprechen: gleiche Parameter
/// (Typ und CHANGING) in gleicher Reihenfolge und gleicher Rückgabetyp. Sonst passt der Aufruf über
/// die Tabelle nicht.
fn same_signature(interface: &InterfaceMethod, method: &MethodInfo<'_>) -> bool {
    interface.params.len() == method.params.len()
        && interface
            .params
            .iter()
            .zip(&method.params)
            .all(|(p, (ty, changing))| p.ty == *ty && p.changing == *changing)
        && match (&interface.ret, method.ret) {
            (None, None) => true,
            (Some(ret), Some(ty)) => ret.ty == ty,
            _ => false,
        }
}

impl<'ctx> Codegen<'ctx> {
    /// CREATE OBJECT name [TYPE klasse] [EXPORTING ...]. Die Klasse ist `TYPE` oder der statische Typ
    /// der Referenz. Ein Objekt bekommt 8 Byte Kopf vor dem Objektzeiger mit der Methodentabelle.
    pub(super) fn emit_create(
        &mut self,
        name: &str,
        class: Option<&str>,
        args: &[NamedArg],
        line: usize,
    ) -> CResult<()> {
        let (slot, ty) = self.data(name, line)?;
        let not_a_ref = || format!("CREATE OBJECT expects an object reference: '{name}'");
        let c = match (class, ty) {
            (Some(class_name), _) => match self.classes.iter().position(|c| c.name == class_name) {
                Some(c) => c,
                None => return err(line, format!("Class '{class_name}' not defined")),
            },
            (None, Ty::Ref(c)) => c,
            (None, Ty::Iface(_)) => {
                return err(
                    line,
                    format!("CREATE OBJECT '{name}': an interface reference needs TYPE <class>"),
                )
            }
            (None, _) => return err(line, not_a_ref()),
        };
        match ty {
            Ty::Ref(static_class) if !self.is_subclass(c, static_class) => {
                return err(
                    line,
                    format!(
                        "Class '{}' is not a subclass of '{}'",
                        self.classes[c].name, self.classes[static_class].name
                    ),
                )
            }
            Ty::Iface(i) if !self.implements(c, i) => {
                return err(
                    line,
                    format!(
                        "Class '{}' does not implement interface '{}'",
                        self.classes[c].name, self.ifaces[i].name
                    ),
                )
            }
            Ty::Ref(_) | Ty::Iface(_) => {}
            _ => return err(line, not_a_ref()),
        }
        if self.classes[c].is_abstract {
            return err(
                line,
                format!(
                    "Class '{}' is abstract and cannot be instantiated",
                    self.classes[c].name
                ),
            );
        }

        let st = self.classes[c].struct_idx;
        let size = self.size_of(Ty::Struct(st)) as usize + OBJECT_HEADER;
        let base = self.call_ptr(self.rt.obj_new, &[self.len(size).into()])?;
        llvm(
            self.builder
                .build_store(base, self.vtables[c].as_pointer_value()),
        )?;
        let obj = unsafe {
            llvm(self.builder.build_gep(
                self.ctx.i8_type(),
                base,
                &[self.len(OBJECT_HEADER)],
                "obj",
            ))?
        };
        self.init_value(&self.builder, obj, Ty::Struct(st))?;
        llvm(self.builder.build_store(slot, obj))?;
        if self.has_method(c, "constructor") {
            self.emit_call_on(Some(obj), c, None, "constructor", args, line)?;
        } else if !args.is_empty() {
            return err(
                line,
                format!("CREATE OBJECT: class has no constructor for '{name}'"),
            );
        }
        Ok(())
    }

    /// Offsets der Interfaces, Interface-Liste der Klassen und die Methodentabellen je Klasse.
    /// Eine Klasse erbt die Tabelle ihrer Oberklasse (die Oberklasse steht im Programm vor ihr).
    pub(super) fn build_vtables(&mut self, ir: &Ir) -> CResult<()> {
        self.ifaces = ir.interfaces.clone();
        self.iface_offsets.clear();
        let mut offset = 0;
        for iface in &ir.interfaces {
            self.iface_offsets.push(offset);
            offset += iface.methods.len();
        }

        let mut tables: Vec<Vec<Option<FunctionValue<'ctx>>>> = Vec::new();
        for (ci, class) in ir.classes.iter().enumerate() {
            let parent = self.classes[ci].parent.filter(|&p| p < ci);

            // Implementierte Interfaces: eigene (mit Unter-Interfaces) und die der Oberklasse
            let mut implements = parent
                .map(|p| self.classes[p].implements.clone())
                .unwrap_or_default();
            for reference in &class.interfaces {
                if let InterfaceRef::Internal(name) = reference {
                    if let Some(i) = ir.interfaces.iter().position(|d| &d.name == name) {
                        add_iface_closure(&ir.interfaces, i, &mut implements);
                    }
                }
            }
            self.classes[ci].implements = implements;

            // Tabelle: Kopie der Oberklasse, eigene Implementierungen überschreiben die Slots
            let mut entries = parent.map(|p| tables[p].clone()).unwrap_or_default();
            for method in self.classes[ci].methods.clone() {
                if method.is_static || !method.name.contains('~') {
                    continue;
                }
                for (ii, iface) in ir.interfaces.iter().enumerate() {
                    let Some(k) = iface.methods.iter().position(|m| m.name == method.name) else {
                        continue;
                    };
                    let interface_method = &iface.methods[k];
                    if interface_method.is_static {
                        continue;
                    }
                    if !same_signature(interface_method, &method) {
                        let line = class
                            .methods
                            .iter()
                            .find(|m| m.name == method.name)
                            .map_or(0, |m| m.line);
                        return err(
                            line,
                            format!(
                                "Method '{}' of class '{}' does not match the signature of the interface method",
                                method.name, class.name
                            ),
                        );
                    }
                    let slot = self.iface_offsets[ii] + k;
                    if entries.len() <= slot {
                        entries.resize(slot + 1, None);
                    }
                    entries[slot] = Some(method.f);
                }
            }
            tables.push(entries);
        }

        self.vtables.clear();
        for (ci, entries) in tables.iter().enumerate() {
            let values: Vec<PointerValue<'ctx>> = entries
                .iter()
                .map(|entry| match entry {
                    Some(f) => f.as_global_value().as_pointer_value(),
                    None => self.ptrt.const_null(),
                })
                .collect();
            let init = self.ptrt.const_array(&values);
            let global = self.module.add_global(
                self.ptrt.array_type(entries.len() as u32),
                None,
                &format!("abap_vt_{}", ir.classes[ci].name),
            );
            global.set_initializer(&init);
            global.set_constant(true);
            self.vtables.push(global);
        }
        Ok(())
    }

    /// Implementiert die Klasse das Interface (auch geerbt oder über ein Unter-Interface)?
    pub(super) fn implements(&self, class: usize, iface: usize) -> bool {
        self.classes[class].implements.contains(&iface)
    }

    /// Ist `class` gleich `of` oder eine Unterklasse davon?
    pub(super) fn is_subclass(&self, class: usize, of: usize) -> bool {
        let mut current = Some(class);
        while let Some(index) = current {
            if index == of {
                return true;
            }
            current = self.classes[index].parent;
        }
        false
    }

    /// Objekt hinter einer Interface-Referenz (einfacher Name). `None`, wenn der Name keine
    /// Interface-Referenz ist; die Fehlermeldung kommt dann aus dem Objektzugriff.
    pub(super) fn iface_object(
        &self,
        name: &str,
        line: usize,
    ) -> CResult<Option<(PointerValue<'ctx>, usize)>> {
        if name == "me" {
            return Ok(None);
        }
        let Ok((slot, Ty::Iface(i))) = self.data_base(name, line) else {
            return Ok(None);
        };
        let obj = llvm(self.builder.build_load(self.ptrt, slot, "iface_obj"))?.into_pointer_value();
        self.check_bound_object(obj)?;
        Ok(Some((obj, i)))
    }

    /// Lädt den Zeiger auf die Methodentabelle aus dem Kopf vor dem Objekt.
    pub(super) fn object_vtable(&self, obj: PointerValue<'ctx>) -> CResult<PointerValue<'ctx>> {
        let header = unsafe {
            llvm(self.builder.build_gep(
                self.ctx.i8_type(),
                obj,
                &[self.i64t.const_int((-(OBJECT_HEADER as i64)) as u64, true)],
                "vt_header",
            ))?
        };
        Ok(llvm(self.builder.build_load(self.ptrt, header, "vtable"))?.into_pointer_value())
    }

    /// Methodenaufruf über eine Interface-Referenz: Slot aus der Tabelle des Objekts, indirekter Aufruf.
    /// Die Parameter werden wie bei Methoden der Klasse übergeben (Zeiger, Rückgabe am Ende).
    pub(super) fn emit_iface_call(
        &mut self,
        obj: PointerValue<'ctx>,
        i: usize,
        method: &str,
        args: &[NamedArg],
        line: usize,
    ) -> CResult<Option<Val<'ctx>>> {
        let iface = self.ifaces[i].clone();
        let qualified = if method.contains('~') {
            method.to_string()
        } else {
            format!("{}~{}", iface.name, method)
        };
        let Some(k) = iface.methods.iter().position(|m| m.name == qualified) else {
            return err(
                line,
                format!(
                    "Method '{method}' is not a method of interface '{}'",
                    iface.name
                ),
            );
        };
        let im = iface.methods[k].clone();
        if im.is_static {
            return err(
                line,
                format!(
                    "Static method '{method}' of interface '{}' cannot be called through a reference (not supported yet)",
                    iface.name
                ),
            );
        }
        let slot = self.iface_offsets[i] + k;
        let table = self.object_vtable(obj)?;
        let entry = unsafe {
            llvm(
                self.builder
                    .build_gep(self.ptrt, table, &[self.len(slot)], "vt_slot"),
            )?
        };
        let func = llvm(self.builder.build_load(self.ptrt, entry, "vt_fn"))?.into_pointer_value();

        let pnames: Vec<String> = im.params.iter().map(|p| p.name.clone()).collect();
        let params: Vec<(Ty, bool)> = im.params.iter().map(|p| (p.ty, p.changing)).collect();
        let mut ptrs: Vec<BasicMetadataValueEnum<'ctx>> = vec![obj.into()];
        ptrs.extend(self.bind_method_args(&pnames, &params, method, args, line)?);

        let mut fn_params: Vec<BasicMetadataTypeEnum<'ctx>> = vec![self.ptrt.into()];
        fn_params.extend(
            params
                .iter()
                .map(|_| BasicMetadataTypeEnum::from(self.ptrt)),
        );
        if im.ret.is_some() {
            fn_params.push(self.ptrt.into());
        }
        let fn_type = self.ctx.void_type().fn_type(&fn_params, false);

        match &im.ret {
            None => {
                llvm(self.builder.build_indirect_call(fn_type, func, &ptrs, ""))?;
                Ok(None)
            }
            Some(ret) => {
                let rty = ret.ty;
                let tmp = llvm(self.builder.build_alloca(self.llvm_ty(rty), "ret_tmp"))?;
                self.init_value(&self.builder, tmp, rty)?;
                ptrs.push(tmp.into());
                llvm(self.builder.build_indirect_call(fn_type, func, &ptrs, ""))?;
                Ok(Some(self.load_val(tmp, rty, line)?))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interface(name: &str, parents: Vec<usize>) -> InterfaceDef {
        InterfaceDef {
            name: name.to_string(),
            methods: Vec::new(),
            parents,
        }
    }

    #[test]
    fn implemented_closure_adds_sub_interfaces_once() {
        let ifaces = vec![interface("if_base", vec![]), interface("if_top", vec![0])];
        let mut out = Vec::new();
        add_iface_closure(&ifaces, 1, &mut out);
        add_iface_closure(&ifaces, 0, &mut out);
        assert_eq!(out, vec![1, 0]);
    }
}
