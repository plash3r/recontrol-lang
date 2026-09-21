use crate::ast::{BinaryOp, Literal, UnaryOp};
use crate::hir::{FunctionId, BUILTIN_LEN_ID, BUILTIN_PRINT_ID, BUILTIN_PRINTLN_ID, BUILTIN_TYPEOF_ID};
use crate::mir::{MirFunction, MirProgram, MirStatement, Operand, Place, Rvalue, Terminator};
use crate::types::Type;
use std::collections::HashMap;
use std::fmt::Write;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodegenError { pub function: String, pub message: String }

pub struct LlvmBackend;

struct Cx<'a> {
    program: &'a MirProgram,
    function: &'a MirFunction,
    locals: HashMap<usize, Type>,
    next: usize,
    strings: Vec<(String, Vec<u8>)>,
    pending: String,
    array_lengths: HashMap<usize, usize>,
}

impl LlvmBackend {
    pub fn emit(program: &MirProgram) -> Result<String, Vec<CodegenError>> {
        let mut module = String::from("; Recontrol Lang LLVM IR\nsource_filename = \"recontrol\"\n\ndeclare void @rcl_println(ptr)\ndeclare void @rcl_print(ptr)\ndeclare void @rcl_print_i8(i8)\ndeclare void @rcl_println_i8(i8)\ndeclare void @rcl_print_i32(i32)\ndeclare void @rcl_println_i32(i32)\ndeclare void @rcl_check_bounds_i32(i32, i32)\n\n");
        let mut strings = Vec::new();
        let mut functions = String::new();
        let mut errors = Vec::new();

        for structure in &program.structs {
            let fields = structure.fields.iter().map(|(_, ty)| llvm_type(ty)).collect::<Vec<_>>().join(", ");
            writeln!(module, "%{} = type {{ {} }}", structure.name, fields).unwrap();
        }
        if !program.structs.is_empty() { module.push('\n'); }

        for function in &program.functions {
            let mut cx = Cx {
                program,
                function,
                locals: function.locals.iter().map(|l| (l.id, l.ty.clone())).collect(),
                next: 0,
                strings: Vec::new(),
                pending: String::new(),
                array_lengths: HashMap::new(),
            };
            match cx.emit_function() {
                Ok(text) => functions.push_str(&text),
                Err(mut e) => errors.append(&mut e),
            }
            for item in cx.strings {
                if !strings.iter().any(|(n, _): &(String, Vec<u8>)| n == &item.0) {
                    strings.push(item);
                }
            }
        }
        if !errors.is_empty() { return Err(errors); }
        for (name, bytes) in strings {
            writeln!(module, "{name} = private unnamed_addr constant [{} x i8] c\"{}\", align 1", bytes.len(), escape_bytes(&bytes)).unwrap();
        }
        if !functions.is_empty() { module.push('\n'); }
        module.push_str(&functions);
        Ok(module)
    }
}

impl<'a> Cx<'a> {
    fn emit_function(&mut self) -> Result<String, Vec<CodegenError>> {
        let ret = self.return_type();
        let params = self.function.locals.iter().take(self.function.param_count)
            .map(|l| format!("{} %arg{}", llvm_type(&l.ty), l.id)).collect::<Vec<_>>().join(", ");
        let mut out = String::new();
        writeln!(out, "define {} @{}({}) {{", llvm_type(&ret), llvm_name(&self.function.name), params).unwrap();
        out.push_str("entry:\n");
        for l in &self.function.locals {
            if l.ty == Type::Unit { return Err(vec![self.err("unit locals are not supported by LLVM backend")]); }
            writeln!(out, "  %l{} = alloca {}", l.id, llvm_type(&l.ty)).unwrap();
        }
        for l in self.function.locals.iter().take(self.function.param_count) {
            writeln!(out, "  store {} %arg{}, ptr %l{}", llvm_type(&l.ty), l.id, l.id).unwrap();
        }
        for block in &self.function.blocks {
            if block.id != 0 { writeln!(out, "bb{}:", block.id).unwrap(); }
            for s in &block.statements { out.push_str(&self.statement(s)?); }
            out.push_str(&self.terminator(&block.terminator)?);
        }
        out.push_str("}\n\n");
        Ok(out)
    }

    fn statement(&mut self, s: &MirStatement) -> Result<String, Vec<CodegenError>> {
        match s {
            MirStatement::StorageLive(_) | MirStatement::StorageDead(_) => Ok(String::new()),
            MirStatement::Assign { place, rvalue } => {
                let ty = self.place_type(place)?;
                let v = self.assignment_rvalue(rvalue, &ty)?;
                if let (Place::Local(local), Rvalue::Array(values)) = (place, rvalue) {
                    self.array_lengths.insert(*local, values.len());
                }
                let p = self.place(place)?;
                let pending = self.take_pending();
                Ok(format!("{pending}  store {} {}, ptr {}\n", llvm_type(&ty), v, p))
            }
            MirStatement::Evaluate(rvalue) => {
                let _value = self.rvalue(rvalue)?;
                Ok(self.take_pending())
            }
        }
    }

    fn terminator(&mut self, t: &Terminator) -> Result<String, Vec<CodegenError>> {
        match t {
            Terminator::Goto(id) => Ok(format!("  br label %bb{id}\n")),
            Terminator::SwitchBool { condition, then_block, else_block } => {
                let c = self.operand(condition)?;
                let pending = self.take_pending();
                Ok(format!("{pending}  br i1 {c}, label %bb{then_block}, label %bb{else_block}\n"))
            }
            Terminator::Return(None) => {
                if self.function.name == "main" && self.return_type_of(self.function) == Type::Unit {
                    Ok("  ret i32 0\n".into())
                } else if self.return_type() == Type::Unit {
                    Ok("  ret void\n".into())
                } else {
                    Err(vec![self.err("bare return in non-unit function")])
                }
            }
            Terminator::Return(Some(v)) => {
                let ty = self.return_type();
                if ty == Type::Unit { return Err(vec![self.err("value returned from unit function")]); }
                let value = self.rvalue(v)?;
                let pending = self.take_pending();
                Ok(format!("{pending}  ret {} {}\n", llvm_type(&ty), value))
            }
            Terminator::Unreachable => Ok("  unreachable\n".into()),
        }
    }

    fn rvalue(&mut self, v: &Rvalue) -> Result<String, Vec<CodegenError>> {
        match v {
            Rvalue::Use(o) => self.operand(o),
            Rvalue::Unary { op, operand } => {
                let x = self.operand(operand)?;
                let ty = self.operand_type(operand)?;
                match op {
                    UnaryOp::Plus => Ok(x),
                    UnaryOp::Minus => {
                        let n=self.tmp();
                        let ins = if is_float(&ty) {
                            format!("%{n} = fneg {} {x}", llvm_type(&ty))
                        } else {
                            format!("%{n} = sub {} 0, {x}", llvm_type(&ty))
                        };
                        writeln!(self.pending, "  {ins}").unwrap();
                        Ok(format!("%{n}"))
                    }
                    UnaryOp::Not => {
                        let n=self.tmp();
                        writeln!(self.pending, "  %{n} = xor i1 {x}, true").unwrap();
                        Ok(format!("%{n}"))
                    }
                    UnaryOp::BorrowShared | UnaryOp::BorrowMutable => self.err_result("borrow rvalue unsupported"),
                }
            }
            Rvalue::Binary { left, op, right } => self.binary(left,*op,right),
            Rvalue::Ref { place, .. } => self.place(place),
            Rvalue::Call { callee, args } => self.call(callee,args),
            Rvalue::Aggregate { name, fields } => {
                let Some(structure) = self.program.structs.iter().find(|structure| structure.name == *name).cloned() else {
                    return self.err_result("unknown struct aggregate");
                };
                let mut value = "undef".to_string();
                for (field_name, operand) in fields {
                    let Some(index) = structure.fields.iter().position(|(field, _)| field == field_name) else {
                        return self.err_result("unknown struct field");
                    };
                    let field_value = self.operand(operand)?;
                    let field_type = &structure.fields[index].1;
                    let next = self.tmp();
                    writeln!(self.pending, "  %{next} = insertvalue %{} {}, {} {}, {}", name, value, llvm_type(field_type), field_value, index).unwrap();
                    value = format!("%{next}");
                }
                Ok(value)
            }
            Rvalue::Array(values) => {
                let element_type = values.first().map(|value| self.operand_type(value)).transpose()?.unwrap_or(Type::Unknown);
                let array_type = format!("[{} x {}]", values.len(), llvm_type(&element_type));
                let storage = self.tmp();
                writeln!(self.pending, "  %{storage} = alloca {array_type}").unwrap();
                let first = self.tmp();
                writeln!(self.pending, "  %{first} = getelementptr inbounds {array_type}, ptr %{storage}, i32 0, i32 0").unwrap();
                for (index, value) in values.iter().enumerate() {
                    let rendered = self.operand(value)?;
                    let slot = self.tmp();
                    writeln!(self.pending, "  %{slot} = getelementptr inbounds {}, ptr %{first}, i32 {}", llvm_type(&element_type), index).unwrap();
                    writeln!(self.pending, "  store {} {}, ptr %{slot}", llvm_type(&element_type), rendered).unwrap();
                }
                Ok(format!("%{first}"))
            }
        }
    }

    fn assignment_rvalue(&mut self, v: &Rvalue, expected: &Type) -> Result<String, Vec<CodegenError>> {
        if let Rvalue::Unary { op: UnaryOp::Minus, operand: Operand::Constant(Literal::Number(n)) } = v {
            let t = self.tmp();
            writeln!(self.pending, "  %{t} = sub {} 0, {}", llvm_type(expected), split_number(n).0).unwrap();
            return Ok(format!("%{t}"));
        }
        self.rvalue(v)
    }

    fn binary(&mut self, l: &Operand, op: BinaryOp, r: &Operand) -> Result<String, Vec<CodegenError>> {
        let a=self.operand(l)?; let b=self.operand(r)?; let ty=self.operand_type(l)?; let t=self.tmp(); let q=llvm_type(&ty);
        let s=if is_float(&ty) {
            match op {
                BinaryOp::Add=>format!("fadd {q} {a}, {b}"), BinaryOp::Subtract=>format!("fsub {q} {a}, {b}"),
                BinaryOp::Multiply=>format!("fmul {q} {a}, {b}"), BinaryOp::Divide=>format!("fdiv {q} {a}, {b}"),
                BinaryOp::Modulo=>format!("frem {q} {a}, {b}"), BinaryOp::Equal=>format!("fcmp oeq {q} {a}, {b}"),
                BinaryOp::NotEqual=>format!("fcmp one {q} {a}, {b}"), BinaryOp::Less=>format!("fcmp olt {q} {a}, {b}"),
                BinaryOp::LessEqual=>format!("fcmp ole {q} {a}, {b}"), BinaryOp::Greater=>format!("fcmp ogt {q} {a}, {b}"),
                BinaryOp::GreaterEqual=>format!("fcmp oge {q} {a}, {b}"), BinaryOp::And|BinaryOp::Or=>return self.err_result("logical operator on float"),
            }
        } else {
            let u=if unsigned(&ty){"u"}else{"s"};
            match op {
                BinaryOp::Add=>format!("add {q} {a}, {b}"), BinaryOp::Subtract=>format!("sub {q} {a}, {b}"),
                BinaryOp::Multiply=>format!("mul {q} {a}, {b}"), BinaryOp::Divide=>format!("{u}div {q} {a}, {b}"),
                BinaryOp::Modulo=>format!("{u}rem {q} {a}, {b}"), BinaryOp::Equal=>format!("icmp eq {q} {a}, {b}"),
                BinaryOp::NotEqual=>format!("icmp ne {q} {a}, {b}"), BinaryOp::Less=>format!("icmp {u}lt {q} {a}, {b}"),
                BinaryOp::LessEqual=>format!("icmp {u}le {q} {a}, {b}"), BinaryOp::Greater=>format!("icmp {u}gt {q} {a}, {b}"),
                BinaryOp::GreaterEqual=>format!("icmp {u}ge {q} {a}, {b}"), BinaryOp::And=>format!("and i1 {a}, {b}"),
                BinaryOp::Or=>format!("or i1 {a}, {b}"),
            }
        };
        writeln!(self.pending, "  %{t} = {s}").unwrap();
        Ok(format!("%{t}"))
    }

    fn call(&mut self, callee: &Operand, args: &[Operand]) -> Result<String, Vec<CodegenError>> {
        let Operand::Function(id)=callee else { return self.err_result("indirect calls are not yet supported"); };
        if *id == BUILTIN_TYPEOF_ID {
            if args.len() != 1 { return self.err_result("typeof expects one argument"); }
            let type_name = self.operand_type(&args[0])?.display_name();
            let bytes = type_name.as_bytes().iter().copied().chain([0]).collect::<Vec<_>>();
            let name = self.intern_string(bytes.clone());
            return Ok(format!("getelementptr inbounds ([{} x i8], ptr {}, i64 0, i64 0)", bytes.len(), name));
        }
        if *id == BUILTIN_LEN_ID {
            if args.len() != 1 { return self.err_result("len expects one argument"); }
            let (Operand::Copy(place) | Operand::Move(place)) = &args[0] else {
                return self.err_result("len expects an array value");
            };
            return Ok(self.array_length(place)?.to_string());
        }
        if *id == BUILTIN_PRINT_ID || *id == BUILTIN_PRINTLN_ID {
            if args.is_empty() { return self.err_result("print/println expects at least one argument"); }
            if args.len() == 1 {
                if let Operand::Copy(place) | Operand::Move(place) = &args[0] {
                    if matches!(self.operand_type(&args[0])?, Type::Array { .. }) {
                        self.emit_print_array(place, *id == BUILTIN_PRINTLN_ID)?;
                        return Ok(String::new());
                    }
                }
            }
            if let Operand::Constant(Literal::String(format)) = &args[0] {
                if args.len() > 1 && (format.contains('%') || format.contains("{}")) {
                    return self.formatted_print(format, &args[1..], *id == BUILTIN_PRINTLN_ID);
                }
            }
            for (index, argument) in args.iter().enumerate() {
                self.emit_print_value(argument, *id == BUILTIN_PRINTLN_ID && index + 1 == args.len())?;
            }
            return Ok(String::new());
        }
        let (name,ret,params)=self.signature(*id)?;
        let mut rendered=Vec::new();
        for (i,a) in args.iter().enumerate() {
            let ty=params.get(i).cloned().unwrap_or(self.operand_type(a)?);
            let v=self.operand(a)?;
            rendered.push(format!("{} {}",llvm_type(&ty),v));
        }
        let text=rendered.join(", ");
        if ret==Type::Unit {
            writeln!(self.pending, "  call void @{name}({text})").unwrap();
            Ok(String::new())
        } else {
            let t=self.tmp();
            writeln!(self.pending, "  %{t} = call {} @{name}({text})",llvm_type(&ret)).unwrap();
            Ok(format!("%{t}"))
        }
    }

    fn formatted_print(&mut self, format: &str, args: &[Operand], newline: bool) -> Result<String, Vec<CodegenError>> {
        let mut text = String::new();
        let mut argument_index = 0;
        let chars: Vec<char> = format.chars().collect();
        let mut index = 0;
        while index < chars.len() {
            let placeholder = if chars[index] == '%' && index + 1 < chars.len() && matches!(chars[index + 1], 'd' | 'i' | 's') {
                index += 2;
                true
            } else if chars[index] == '{' && index + 1 < chars.len() && chars[index + 1] == '}' {
                index += 2;
                true
            } else {
                text.push(chars[index]);
                index += 1;
                false
            };
            if placeholder {
                self.emit_print_string(&text, false)?;
                text.clear();
                let Some(argument) = args.get(argument_index) else { return self.err_result("not enough arguments for format string"); };
                self.emit_print_value(argument, false)?;
                argument_index += 1;
            }
        }
        self.emit_print_string(&text, newline)?;
        if argument_index != args.len() { return self.err_result("too many arguments for format string"); }
        if text.is_empty() && !newline && argument_index == 0 { self.emit_print_string("", false)?; }
        Ok(String::new())
    }

    fn emit_print_string(&mut self, value: &str, newline: bool) -> Result<(), Vec<CodegenError>> {
        let literal = Operand::Constant(Literal::String(value.to_string()));
        self.emit_print_value(&literal, newline)
    }

    fn emit_print_value(&mut self, argument: &Operand, newline: bool) -> Result<(), Vec<CodegenError>> {
        let ty = self.operand_type(argument)?;
        let name = match (&ty, newline) {
            (Type::Str, false) => "rcl_print",
            (Type::Str, true) => "rcl_println",
            (Type::I8, false) => "rcl_print_i8",
            (Type::I8, true) => "rcl_println_i8",
            (Type::I32, false) => "rcl_print_i32",
            (Type::I32, true) => "rcl_println_i32",
            _ => return self.err_result("print/println supports str, i8, and i32 values"),
        };
        let value = self.operand(argument)?;
        writeln!(self.pending, "  call void @{name}({} {value})", llvm_type(&ty)).unwrap();
        Ok(())
    }

    fn emit_print_array(&mut self, place: &Place, newline: bool) -> Result<(), Vec<CodegenError>> {
        let Type::Array { element: element_type, .. } = self.place_type(place)? else {
            return self.err_result("print array expects an array value");
        };
        let length = self.array_length(place)?;
        let source = self.place(place)?;
        let base = self.tmp();
        writeln!(self.pending, "  %{base} = load ptr, ptr {source}").unwrap();
        for index in 0..length {
            if index > 0 { self.emit_print_string(" ", false)?; }
            let slot = self.tmp();
            writeln!(self.pending, "  %{slot} = getelementptr inbounds {}, ptr %{base}, i32 {}", llvm_type(&element_type), index).unwrap();
            let value = self.tmp();
            writeln!(self.pending, "  %{value} = load {}, ptr %{slot}", llvm_type(&element_type)).unwrap();
            self.emit_print_typed_value(&element_type, &format!("%{value}"), false)?;
        }
        if newline {
            self.emit_print_string("", true)
        } else {
            Ok(())
        }
    }

    fn emit_print_typed_value(&mut self, ty: &Type, value: &str, newline: bool) -> Result<(), Vec<CodegenError>> {
        let name = match (ty, newline) {
            (Type::Str, false) => "rcl_print",
            (Type::Str, true) => "rcl_println",
            (Type::I8, false) => "rcl_print_i8",
            (Type::I8, true) => "rcl_println_i8",
            (Type::I32, false) => "rcl_print_i32",
            (Type::I32, true) => "rcl_println_i32",
            _ => return self.err_result("array printing supports str, i8, and i32 elements"),
        };
        writeln!(self.pending, "  call void @{name}({} {value})", llvm_type(ty)).unwrap();
        Ok(())
    }

    fn operand(&mut self, o: &Operand) -> Result<String, Vec<CodegenError>> {
        match o {
            Operand::Constant(x)=>self.literal(x),
            Operand::Function(id)=>{
                if *id==BUILTIN_PRINT_ID || *id==BUILTIN_PRINTLN_ID { self.err_result("builtin used as a value") }
                else { Ok(format!("@{}",self.signature(*id)?.0)) }
            }
            Operand::Copy(p)|Operand::Move(p)=>{
                let ty=self.place_type(p)?; let p=self.place(p)?; let t=self.tmp();
                writeln!(self.pending, "  %{t} = load {}, ptr {p}",llvm_type(&ty)).unwrap();
                Ok(format!("%{t}"))
            }
        }
    }

    fn literal(&mut self, x: &Literal) -> Result<String, Vec<CodegenError>> {
        match x {
            Literal::Bool(v)=>Ok(if *v{"true".into()}else{"false".into()}),
            Literal::Number(n)=>Ok(split_number(n).0.to_string()),
            Literal::String(s)=>{
                let bytes=s.as_bytes().iter().copied().chain([0]).collect::<Vec<_>>();
                let name=self.intern_string(bytes.clone());
                Ok(format!("getelementptr inbounds ([{} x i8], ptr {}, i64 0, i64 0)",bytes.len(),name))
            }
        }
    }

    fn place(&mut self,p:&Place)->Result<String,Vec<CodegenError>> {
        match p {
            Place::Local(id) => Ok(format!("%l{id}")),
            Place::Field { base, name } => {
                let base_type = self.place_type(base)?;
                let (struct_name, base_pointer) = match base_type {
                    Type::Named(struct_name) => (struct_name, self.place(base)?),
                    Type::Reference { inner, .. } => {
                        let Type::Named(struct_name) = *inner else { return self.err_result("field base is not a struct"); };
                        let source = self.place(base)?;
                        let temp = self.tmp();
                        writeln!(self.pending, "  %{temp} = load ptr, ptr {source}").unwrap();
                        (struct_name, format!("%{temp}"))
                    }
                    _ => return self.err_result("field base is not a struct"),
                };
                let Some(structure) = self.program.structs.iter().find(|structure| structure.name == struct_name).cloned() else {
                    return self.err_result("unknown struct type");
                };
                let Some(index) = structure.fields.iter().position(|(field, _)| field == name) else {
                    return self.err_result("unknown struct field");
                };
                let temp = self.tmp();
                writeln!(self.pending, "  %{temp} = getelementptr inbounds %{}, ptr {}, i32 0, i32 {}", struct_name, base_pointer, index).unwrap();
                Ok(format!("%{temp}"))
            }
            Place::Index { base, index } => {
                let element_type = self.place_type(p)?;
                let base_place = self.place(base)?;
                let base_pointer = self.tmp();
                writeln!(self.pending, "  %{base_pointer} = load ptr, ptr {base_place}").unwrap();
                let index_value = self.operand(index)?;
                let index_type = self.operand_type(index)?;
                let index_i32 = if index_type == Type::I32 {
                    index_value.clone()
                } else {
                    let converted = self.tmp();
                    let instruction = if unsigned(&index_type) { "zext" } else { "sext" };
                    writeln!(self.pending, "  %{converted} = {instruction} {} {index_value} to i32", llvm_type(&index_type)).unwrap();
                    format!("%{converted}")
                };
                let length = self.array_length(base)?;
                writeln!(self.pending, "  call void @rcl_check_bounds_i32(i32 {index_i32}, i32 {length})").unwrap();
                let slot = self.tmp();
                writeln!(self.pending, "  %{slot} = getelementptr inbounds {}, ptr %{base_pointer}, i32 {index_i32}", llvm_type(&element_type)).unwrap();
                Ok(format!("%{slot}"))
            }
        }
    }

    fn array_length(&self, place: &Place) -> Result<usize, Vec<CodegenError>> {
        match place {
            Place::Local(local) => {
                if let Some(length) = self.array_lengths.get(local).copied() {
                    return Ok(length);
                }
                match self.locals.get(local) {
                    Some(Type::Array { len, .. }) => Ok(*len),
                    _ => Err(vec![self.err("array length is unavailable")]),
                }
            }
            _ => match self.place_type(place)? {
                Type::Array { len, .. } => Ok(len),
                _ => Err(vec![self.err("array length is unavailable for this expression")]),
            },
        }
    }

    fn place_type(&self,p:&Place)->Result<Type,Vec<CodegenError>> {
        match p {
            Place::Local(id) => self.locals.get(id).cloned().ok_or_else(||vec![self.err("unknown local")]),
            Place::Field { base, name } => {
                let base_type = self.place_type(base)?;
                let type_name = match base_type {
                    Type::Named(name) => name,
                    Type::Reference { inner, .. } => match *inner { Type::Named(name) => name, _ => return self.err_result("field base is not a struct") },
                    _ => return self.err_result("field base is not a struct"),
                };
                self.program.structs.iter().find(|structure| structure.name == type_name)
                    .and_then(|structure| structure.fields.iter().find(|(field, _)| field == name).map(|(_, ty)| ty.clone()))
                    .ok_or_else(|| vec![self.err("unknown struct field")])
            }
            Place::Index { base, .. } => match self.place_type(base)? {
                Type::Array { element, .. } => Ok(*element),
                _ => self.err_result("index base is not an array"),
            },
        }
    }

    fn operand_type(&self,o:&Operand)->Result<Type,Vec<CodegenError>> {
        match o {
            Operand::Copy(p)|Operand::Move(p)=>self.place_type(p),
            Operand::Constant(Literal::Bool(_))=>Ok(Type::Bool),
            Operand::Constant(Literal::String(_))=>Ok(Type::Str),
            Operand::Constant(Literal::Number(n))=>Ok(number_type(n)),
            Operand::Function(_)=>self.err_result("function has no scalar type"),
        }
    }

    fn signature(&self,id:FunctionId)->Result<(String,Type,Vec<Type>),Vec<CodegenError>> {
        if id==BUILTIN_PRINTLN_ID { return Ok(("rcl_println".into(),Type::Unit,vec![Type::Str])); }
        if id==BUILTIN_PRINT_ID { return Ok(("rcl_print".into(),Type::Unit,vec![Type::Str])); }
        if id==BUILTIN_TYPEOF_ID { return Ok(("rcl_typeof".into(),Type::Str,vec![Type::Unknown])); }
        let f=self.program.functions.get(id).ok_or_else(||vec![self.err("invalid function ID")])?;
        Ok((llvm_name(&f.name),self.return_type_of(f),f.locals.iter().take(f.param_count).map(|l|l.ty.clone()).collect()))
    }

    fn return_type(&self)->Type {
        let ty = self.return_type_of(self.function);
        // A native process entry point must return an integer status code.
        // RCL allows a unit-returning `main`, so lower that case to `i32 0`.
        if self.function.name == "main" && ty == Type::Unit { Type::I32 } else { ty }
    }

    fn return_type_of(&self,f:&MirFunction)->Type {
        for b in &f.blocks {
            if let Terminator::Return(Some(v))=&b.terminator {
                if let Some(t)=self.rvalue_type(v,f) { return t; }
            }
        }
        Type::Unit
    }

    fn rvalue_type(&self,v:&Rvalue,f:&MirFunction)->Option<Type> {
        match v {
            Rvalue::Use(o)|Rvalue::Unary{operand:o,..}=>self.static_operand_type(o,f),
            Rvalue::Binary{left,op,..}=>if matches!(op,BinaryOp::Equal|BinaryOp::NotEqual|BinaryOp::Less|BinaryOp::LessEqual|BinaryOp::Greater|BinaryOp::GreaterEqual|BinaryOp::And|BinaryOp::Or){Some(Type::Bool)}else{self.static_operand_type(left,f)},
            Rvalue::Ref{mutable,place}=>self.static_place_type(place,f).map(|inner| Type::Reference { mutable:*mutable, inner:Box::new(inner) }),
            Rvalue::Call{callee:Operand::Function(id),..}=>self.signature(*id).ok().map(|(_,ret,_)|ret),
            _=>None,
        }
    }

    fn static_operand_type(&self,o:&Operand,f:&MirFunction)->Option<Type> {
        match o {
            Operand::Copy(p)|Operand::Move(p)=>self.static_place_type(p,f),
            Operand::Constant(Literal::Bool(_))=>Some(Type::Bool), Operand::Constant(Literal::String(_))=>Some(Type::Str),
            Operand::Constant(Literal::Number(n))=>Some(number_type(n)), Operand::Function(_)=>None,
        }
    }

    fn static_place_type(&self,p:&Place,f:&MirFunction)->Option<Type> {
        match p { Place::Local(id)=>f.locals.iter().find(|l|l.id==*id).map(|l|l.ty.clone()), _=>None }
    }

    fn intern_string(&mut self,bytes:Vec<u8>)->String {
        if let Some((n,_))=self.strings.iter().find(|(_,b)|*b==bytes) { return n.clone(); }
        let mut n="@.str.".to_string(); for b in &bytes { write!(n,"{:02X}",b).unwrap(); } self.strings.push((n.clone(),bytes)); n
    }

    fn tmp(&mut self)->usize { let n=self.next; self.next+=1; n }
    fn take_pending(&mut self)->String { std::mem::take(&mut self.pending) }
    fn err(&self,msg:&str)->CodegenError { CodegenError{function:self.function.name.clone(),message:msg.into()} }
    fn err_result<T>(&self,msg:&str)->Result<T,Vec<CodegenError>> { Err(vec![self.err(msg)]) }
}

fn split_number(n:&str)->(&str,&str) { let i=n.find(|c:char| c.is_ascii_alphabetic()).unwrap_or(n.len()); (&n[..i],&n[i..]) }
fn number_type(n:&str)->Type {
    let l=n.to_ascii_lowercase();
    for s in ["u8","u16","u32","u64","u128","u256","i8","i16","i32","i64","i128","i256","f32","f64","f128"] {
        if l.ends_with(s) { return match s {
            "u8"=>Type::U8,"u16"=>Type::U16,"u32"=>Type::U32,"u64"=>Type::U64,"u128"=>Type::U128,"u256"=>Type::U256,
            "i8"=>Type::I8,"i16"=>Type::I16,"i32"=>Type::I32,"i64"=>Type::I64,"i128"=>Type::I128,"i256"=>Type::I256,
            "f32"=>Type::F32,"f64"=>Type::F64,"f128"=>Type::F128,_=>Type::I32 } }
    }
    if l.contains('.') { Type::F64 } else { Type::I32 }
}
fn is_float(t:&Type)->bool { matches!(t,Type::F32|Type::F64|Type::F128) }
fn unsigned(t:&Type)->bool { matches!(t,Type::U8|Type::U16|Type::U32|Type::U64|Type::U128|Type::U256) }
fn llvm_type(t:&Type)->String { match t {
    Type::I8|Type::U8=>"i8", Type::I16|Type::U16=>"i16", Type::I32|Type::U32|Type::Char=>"i32",
    Type::I64|Type::U64=>"i64", Type::I128|Type::U128=>"i128", Type::I256|Type::U256=>"i256",
    Type::F32=>"float",Type::F64=>"double",Type::F128=>"fp128",Type::Bool=>"i1",Type::Str|Type::Reference{..}=>"ptr",
    Type::Unit=>"void",Type::Named(n)=>return format!("%{}",n),Type::Array { .. }|Type::Unknown=>"ptr"
}.into() }
fn llvm_name(n:&str)->String { if n=="main"{"main".into()}else{format!("rcl_{n}")} }
fn escape_bytes(b:&[u8])->String { let mut s=String::new(); for x in b { match x {92=>s.push_str("\\5C"),34=>s.push_str("\\22"),0=>s.push_str("\\00"),10=>s.push_str("\\0A"),13=>s.push_str("\\0D"),9=>s.push_str("\\09"),32..=126=>s.push(*x as char),_=>write!(s,"\\{:02X}",x).unwrap()} } s }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{hir::HirLowerer, lexer::Lexer, mir::MirLowerer, mir_opt::MirOptimizer, parser::Parser, sema::SemanticAnalyzer};

    #[test]
    fn emits_direct_function_call() {
        let source = "fn add(a:i32,b:i32):i32{return a+b} fn main(){let x:i32=add(1,2)}";
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        SemanticAnalyzer::check(&program).unwrap();
        let hir = HirLowerer::lower(&program);
        let mut mir = MirLowerer::lower(&hir);
        MirOptimizer::optimize(&mut mir);
        let llvm = LlvmBackend::emit(&mir).unwrap();
        assert!(llvm.contains("define i32 @rcl_add"));
        assert!(llvm.contains("call i32 @rcl_add"));
        assert!(!llvm.contains("= load i32, ptr %l0)"));
    }

    #[test]
    fn emits_i256_hpc_workload() {
        let source = r#"
fn gcd(a:i256,b:i256):i256 {
    let mut x:i256=a
    let mut y:i256=b
    while y != 0i256 {
        let mut t:i256=x % y
        x=y
        y=t
    }
    return x
}
fn lcm(a:i256,b:i256):i256 { return (a / gcd(a,b)) * b }
fn modpow(base:i256,exp:i256,modulus:i256):i256 {
    let mut result:i256=1i256
    let mut b:i256=base % modulus
    let mut e:i256=exp
    while e > 0i256 {
        if e % 2i256 == 1i256 { result=(result*b)%modulus }
        b=(b*b)%modulus
        e=e/2i256
    }
    return result
}
fn fibonacci(n:i256):i256 {
    let mut a:i256=0i256
    let mut b:i256=1i256
    let mut i:i256=0i256
    while i < n { let mut t:i256=a+b a=b b=t i=i+1i256 }
    return a
}
fn factorial_mod(n:i256,m:i256):i256 {
    let mut r:i256=1i256
    let mut i:i256=1i256
    while i <= n { r=(r*i)%m i=i+1i256 }
    return r
}
fn arithmetic_sum(n:i256):i256 { return n*(n+1i256)/2i256 }
fn sum_of_squares(n:i256):i256 { return n*(n+1i256)*(2i256*n+1i256)/6i256 }
fn main() {
    let a:i256=123456789012345678901234567890i256
    let b:i256=98765432109876543210987654321i256
    let x:i256=gcd(a,b)
    let y:i256=lcm(a,b)
    let z:i256=modpow(123456789i256,12345i256,1000000007i256)
    let f:i256=fibonacci(100i256)
    let q:i256=factorial_mod(100i256,1000000007i256)
    let s:i256=arithmetic_sum(1000000i256)
    let ss:i256=sum_of_squares(1000000i256)
}
"#;
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        SemanticAnalyzer::check(&program).unwrap();
        let hir = HirLowerer::lower(&program);
        let mut mir = MirLowerer::lower(&hir);
        MirOptimizer::optimize(&mut mir);
        let llvm = LlvmBackend::emit(&mir).unwrap();
        assert!(llvm.contains("define i256 @rcl_gcd"));
        assert!(llvm.contains("define i256 @rcl_modpow"));
        assert!(llvm.contains("srem i256"));
        assert!(llvm.contains("sdiv i256"));
        assert!(llvm.contains("mul i256"));
    }

    #[test]
    fn emits_i256_arithmetic() {
        let source = "fn add(a:i256,b:i256):i256{return a+b} fn main(){let x:i256=add(340282366920938463463374607431768211456i256,2i256)}";
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        SemanticAnalyzer::check(&program).unwrap();
        let hir = HirLowerer::lower(&program);
        let mut mir = MirLowerer::lower(&hir);
        MirOptimizer::optimize(&mut mir);
        let llvm = LlvmBackend::emit(&mir).unwrap();
        assert!(llvm.contains("define i256 @rcl_add"));
        assert!(llvm.contains("add i256"));
    }

    #[test]
    fn emits_hello_world_llvm() {
        let source = r#"fn main(){let message:str="Hello" println(message)}"#;
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        SemanticAnalyzer::check(&program).unwrap();
        let hir = HirLowerer::lower(&program);
        let mut mir = MirLowerer::lower(&hir);
        MirOptimizer::optimize(&mut mir);
        let llvm = LlvmBackend::emit(&mir).unwrap();
        assert!(!llvm.contains("define void @main()"));
        assert!(llvm.contains("@rcl_println"));
        assert!(llvm.contains("Hello"));
        assert!(llvm.contains("define i32 @main()"));
        assert!(llvm.contains("ret i32 0"));
    }

    #[test]
    fn emits_numeric_prints_and_multiple_arguments() {
        let source = r#"fn main(){let x:i32=5 println(typeof(x), x)}"#;
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        SemanticAnalyzer::check(&program).unwrap();
        let hir = HirLowerer::lower(&program);
        let mut mir = MirLowerer::lower(&hir);
        MirOptimizer::optimize(&mut mir);
        let llvm = LlvmBackend::emit(&mir).unwrap();
        assert!(llvm.contains("call void @rcl_print(") || llvm.contains("call void @rcl_println("));
        assert!(llvm.contains("call void @rcl_println_i32"));
    }

    #[test]
    fn emits_array_literal_and_index() {
        let source = r#"fn main(){let values=[10,20,30] println(values[1])}"#;
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        SemanticAnalyzer::check(&program).unwrap();
        let hir = HirLowerer::lower(&program);
        let mut mir = MirLowerer::lower(&hir);
        MirOptimizer::optimize(&mut mir);
        let llvm = LlvmBackend::emit(&mir).unwrap();
        assert!(llvm.contains("alloca [3 x i32]"));
        assert!(llvm.contains("getelementptr inbounds i32"));
        assert!(llvm.contains("call void @rcl_println_i32"));
    }
}
