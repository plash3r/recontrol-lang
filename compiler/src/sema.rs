use std::collections::HashMap;

use crate::ast::*;
use crate::lexer::Span;
use crate::types::Type;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticError { pub message: String, pub span: Span }

#[derive(Debug, Clone)]
struct FunctionSig { params: Vec<Type>, return_type: Type }

#[derive(Debug, Clone)]
struct StructInfo { fields: HashMap<String, Type> }

#[derive(Debug, Default)]
struct Env {
    vars: Vec<HashMap<String, Type>>,
    mutable: Vec<HashMap<String, bool>>,
}
impl Env {
    fn push(&mut self) { self.vars.push(HashMap::new()); self.mutable.push(HashMap::new()); }
    fn pop(&mut self) { self.vars.pop(); self.mutable.pop(); }
    fn define(&mut self, name: String, ty: Type, is_mut: bool) -> bool {
        let vars=self.vars.last_mut().unwrap(); let muts=self.mutable.last_mut().unwrap();
        let fresh=vars.insert(name.clone(),ty).is_none(); if fresh { muts.insert(name,is_mut); } fresh
    }
    fn get(&self,n:&str)->Option<Type>{self.vars.iter().rev().find_map(|s|s.get(n).cloned())}
    fn is_mutable(&self,n:&str)->bool{self.mutable.iter().rev().find_map(|s|s.get(n).copied()).unwrap_or(false)}
}

pub struct SemanticAnalyzer {
    functions: HashMap<String,FunctionSig>,
    structs: HashMap<String,StructInfo>,
    methods: HashMap<(String,String),FunctionSig>,
    errors: Vec<SemanticError>,
}
impl SemanticAnalyzer {
    pub fn check(program:&Program)->Result<(),Vec<SemanticError>>{
        let mut a=Self{functions:HashMap::new(),structs:HashMap::new(),methods:HashMap::new(),errors:Vec::new()};
        a.collect(program); a.check_items(program);
        if a.errors.is_empty(){Ok(())}else{Err(a.errors)}
    }
    fn error(&mut self,msg:impl Into<String>){self.errors.push(SemanticError{message:msg.into(),span:Span{line:1,column:1,length:0}});}
    fn collect(&mut self,p:&Program){
        for item in &p.items { match item {
            Item::Function(f)=>{
                if self.functions.contains_key(&f.name){self.error(format!("duplicate function '{}'",f.name));continue}
                self.functions.insert(f.name.clone(),FunctionSig{params:f.params.iter().map(|p|Type::from_ref(&p.ty)).collect(),return_type:f.return_type.as_ref().map(Type::from_ref).unwrap_or(Type::Unit)});
            }
            Item::Struct(s)=>{
                if self.structs.contains_key(&s.name){self.error(format!("duplicate struct '{}'",s.name));continue}
                let mut fields=HashMap::new();
                for f in &s.fields {if fields.insert(f.name.clone(),Type::from_ref(&f.ty)).is_some(){self.error(format!("duplicate field '{}.{}'",s.name,f.name));}}
                self.structs.insert(s.name.clone(),StructInfo{fields});
            }
            Item::Impl(i)=>{
                if !self.structs.contains_key(&i.type_name){self.error(format!("unknown type '{}' in impl",i.type_name));}
                for f in &i.methods {
                    let params=f.params.iter().map(|p|{
                        if p.name=="self"{Type::Reference{mutable:p.ty.reference==ReferenceKind::Mutable,inner:Box::new(Type::Named(i.type_name.clone()))}}
                        else{Type::from_ref(&p.ty)}
                    }).collect();
                    let key=(i.type_name.clone(),f.name.clone());
                    if self.methods.insert(key.clone(),FunctionSig{params,return_type:f.return_type.as_ref().map(Type::from_ref).unwrap_or(Type::Unit)}).is_some(){self.error(format!("duplicate method '{}.{}'",key.0,key.1));}
                }
            }
        }}
    }
    fn check_items(&mut self,p:&Program){
        for item in &p.items {match item {
            Item::Function(f)=>self.check_function(f,None),
            Item::Impl(i)=>for f in &i.methods{self.check_function(f,Some(&i.type_name));},
            Item::Struct(_)=>{}
        }}
    }
    fn check_function(&mut self,f:&Function,impl_ty:Option<&str>){
        let sig=if let Some(t)=impl_ty{self.methods.get(&(t.to_string(),f.name.clone())).cloned()}else{self.functions.get(&f.name).cloned()};
        let Some(sig)=sig else{return}; let mut env=Env::default(); env.push();
        for (i,p) in f.params.iter().enumerate(){let ty=sig.params[i].clone();let mutability=matches!(&ty,Type::Reference{mutable:true,..});if !env.define(p.name.clone(),ty,mutability){self.error(format!("duplicate parameter '{}'",p.name));}}
        self.check_block(&f.body,&mut env,&sig.return_type);env.pop();
    }
    fn check_block(&mut self,b:&Block,e:&mut Env,r:&Type){e.push();for s in &b.statements{self.check_stmt(s,e,r)}e.pop();}
    fn check_stmt(&mut self,s:&Stmt,e:&mut Env,r:&Type){
        match s {
            Stmt::Let{name,mutable,ty,initializer}=>{
                let actual=initializer.as_ref().map(|x|self.expr(x,e));let declared=ty.as_ref().map(Type::from_ref);
                let final_ty=match(declared,actual){(Some(d),Some(a))=>{if !self.compatible(&d,&a){self.error(format!("type mismatch: expected {}, found {}",d.display_name(),a.display_name()));}d},(Some(d),None)=>d,(None,Some(a))=>a,(None,None)=>Type::Unknown};
                if !e.define(name.clone(),final_ty,*mutable){self.error(format!("duplicate variable '{}'",name));}
            }
            Stmt::Expr(x)=>{self.expr(x,e);}
            Stmt::Return(x)=>{let a=x.as_ref().map(|x|self.expr(x,e)).unwrap_or(Type::Unit);if !self.compatible(r,&a){self.error(format!("return type mismatch: expected {}, found {}",r.display_name(),a.display_name()));}}
            Stmt::If{condition,then_branch,else_branch}=>{let t=self.expr(condition,e);if t!=Type::Bool{self.error(format!("if condition must be bool, found {}",t.display_name()));}self.check_block(then_branch,e,r);if let Some(x)=else_branch{self.check_stmt(x,e,r);}}
            Stmt::While{condition,body}=>{let t=self.expr(condition,e);if t!=Type::Bool{self.error(format!("while condition must be bool, found {}",t.display_name()));}self.check_block(body,e,r);}
            Stmt::DoWhile{body,condition}=>{self.check_block(body,e,r);let t=self.expr(condition,e);if t!=Type::Bool{self.error(format!("do while condition must be bool, found {}",t.display_name()));}}
            Stmt::For{initializer,condition,update,body}=>{e.push();if let Some(x)=initializer{self.check_stmt(x,e,r)}if let Some(x)=condition{let t=self.expr(x,e);if t!=Type::Bool{self.error(format!("for condition must be bool, found {}",t.display_name()));}}if let Some(x)=update{self.expr(x,e)}self.check_block(body,e,r);e.pop();}
            Stmt::Block(b)=>self.check_block(b,e,r),
        }
    }
    fn expr(&mut self,x:&Expr,e:&Env)->Type{
        match x {
            Expr::Literal(Literal::Bool(_))=>Type::Bool,
            Expr::Literal(Literal::String(_))=>Type::Str,
            Expr::Literal(Literal::Number(n))=>self.number_type(n),
            Expr::Identifier(n)=>e.get(n).or_else(||if self.functions.contains_key(n){Some(Type::Named(format!("fn {}",n)))}else{None}).unwrap_or_else(||{self.error(format!("unknown identifier '{}'",n));Type::Unknown}),
            Expr::Grouping(x)=>self.expr(x,e),
            Expr::Unary{op,expr}=>{let t=self.expr(expr,e);match op{
                UnaryOp::Not=>if t==Type::Bool{Type::Bool}else{self.error("operator ! requires bool");Type::Unknown},
                UnaryOp::Plus|UnaryOp::Minus=>if t.is_numeric(){t}else{self.error("unary operator requires number");Type::Unknown},
                UnaryOp::BorrowShared=>Type::Reference{mutable:false,inner:Box::new(t)},
                UnaryOp::BorrowMutable=>{if !self.assignable(expr,e){self.error("cannot mutably borrow immutable expression");}Type::Reference{mutable:true,inner:Box::new(t)}}
            }}
            Expr::Binary{left,op,right}=>{let a=self.expr(left,e);let b=self.expr(right,e);match op{BinaryOp::And|BinaryOp::Or=>{if a!=Type::Bool||b!=Type::Bool{self.error("logical operators require bool operands");}Type::Bool},BinaryOp::Equal|BinaryOp::NotEqual|BinaryOp::Less|BinaryOp::LessEqual|BinaryOp::Greater|BinaryOp::GreaterEqual=>{if !self.compatible(&a,&b){self.error("incompatible comparison types");}Type::Bool},_=>if a.is_numeric()&&b.is_numeric()&&self.compatible(&a,&b){a}else{self.error("incompatible numeric operands");Type::Unknown}}}
            Expr::Assignment{target,op,value}=>{let t=self.expr(target,e);if !self.assignable(target,e){self.error("cannot assign to immutable expression");}let v=self.expr(value,e);if *op==AssignOp::Assign{if !self.compatible(&t,&v){self.error("assignment type mismatch");}}else if !t.is_numeric()||!v.is_numeric()||!self.compatible(&t,&v){self.error("compound assignment requires compatible numeric operands");}t}
            Expr::Call{callee,args}=>self.call(callee,args,e),
            Expr::Member{object,name}=>self.member(object,name,e),
            Expr::Postfix{expr,..}=>{let t=self.expr(expr,e);if !self.assignable(expr,e){self.error("cannot modify immutable expression");}if !t.is_numeric(){self.error("increment/decrement requires a numeric value");}t}
            Expr::StructLiteral{name,fields}=>self.struct_lit(name,fields,e),
            Expr::Array(xs)=>{if xs.is_empty(){Type::Array(Box::new(Type::Unknown))}else{let t=self.expr(&xs[0],e);for x in &xs[1..]{let q=self.expr(x,e);if !self.compatible(&t,&q){self.error("array elements must have compatible types");}}Type::Array(Box::new(t))}}
            Expr::Index{object,index}=>{let o=self.expr(object,e);let i=self.expr(index,e);if !i.is_integer(){self.error("array index must be an integer");}match o{Type::Array(x)=>*x,Type::Str=>Type::Char,_=>{self.error("indexing requires an array or str");Type::Unknown}}}
        }
    }
    fn assignable(&self,x:&Expr,e:&Env)->bool{match x{Expr::Identifier(n)=>e.is_mutable(n),Expr::Member{object,..}|Expr::Index{object,..}=>matches!(object.as_ref(),Expr::Identifier(n) if e.is_mutable(n)),_=>false}}
    fn call(&mut self,c:&Expr,args:&[Expr],e:&Env)->Type{
        if let Expr::Identifier(n)=c{if n=="println"||n=="print"{for a in args{self.expr(a,e);}return Type::Unit}if let Some(s)=self.functions.get(n).cloned(){return self.signature(&s,args,e)}self.error(format!("unknown function '{}'",n));return Type::Unknown}
        if let Expr::Member{object,name}=c{let o=self.expr(object,e);let tn=match o{Type::Named(n)=>n,Type::Reference{inner,..}=>match *inner{Type::Named(n)=>n,_=>String::new()},_=>String::new()};if let Some(s)=self.methods.get(&(tn.clone(),name.clone())).cloned(){let mut all=vec![object.as_ref().clone()];all.extend_from_slice(args);return self.signature(&s,&all,e)}self.error(format!("unknown method '{}.{}'",tn,name));return Type::Unknown}
        self.error("expression is not callable");Type::Unknown
    }
    fn signature(&mut self,s:&FunctionSig,args:&[Expr],e:&Env)->Type{
        if args.len()!=s.params.len(){self.error(format!("wrong argument count: expected {}, found {}",s.params.len(),args.len()));}
        for(i,a)in args.iter().enumerate(){let actual=self.expr(a,e);if let Some(expected)=s.params.get(i){let ok=match expected{Type::Reference{inner,..} if i==0=>self.compatible(inner,&actual),_=>self.compatible(expected,&actual)};if !ok{self.error(format!("argument {} type mismatch: expected {}, found {}",i+1,expected.display_name(),actual.display_name()));}}}s.return_type.clone()
    }
    fn member(&mut self,o:&Expr,n:&str,e:&Env)->Type{
        let t=self.expr(o,e);let tn=match t{Type::Named(x)=>x,Type::Reference{inner,..}=>match *inner{Type::Named(x)=>x,_=>String::new()},_=>String::new()};
        if let Some(s)=self.structs.get(&tn){if let Some(t)=s.fields.get(n){return t.clone();}}
        self.error(format!("unknown member '{}.{}'",tn,n));Type::Unknown
    }
    fn struct_lit(&mut self,n:&str,fs:&[(String,Expr)],e:&Env)->Type{
        let Some(info)=self.structs.get(n).cloned()else{self.error(format!("unknown struct '{}'",n));return Type::Unknown};
        let mut seen=HashMap::new();
        for(f,x)in fs{let a=self.expr(x,e);if seen.insert(f,true).is_some(){self.error(format!("duplicate field '{}.{}'",n,f));}match info.fields.get(f){Some(t)if !self.compatible(t,&a)=>self.error(format!("field '{}.{}' type mismatch",n,f)),None=>self.error(format!("unknown field '{}.{}'",n,f)),_=>{}}}
        for f in info.fields.keys(){if !seen.contains_key(f){self.error(format!("missing field '{}.{}'",n,f));}}
        Type::Named(n.into())
    }
    fn number_type(&self,n:&str)->Type{let l=n.to_ascii_lowercase();for s in ["u8","u16","u32","u64","u128","u256","i8","i16","i32","i64","i128","i256","f32","f64","f128"]{if l.ends_with(s){return match s{"u8"=>Type::U8,"u16"=>Type::U16,"u32"=>Type::U32,"u64"=>Type::U64,"u128"=>Type::U128,"u256"=>Type::U256,"i8"=>Type::I8,"i16"=>Type::I16,"i32"=>Type::I32,"i64"=>Type::I64,"i128"=>Type::I128,"i256"=>Type::I256,"f32"=>Type::F32,"f64"=>Type::F64,"f128"=>Type::F128,_=>Type::Unknown}}}if l.contains('.') {Type::F64}else{Type::I32}}
    fn compatible(&self,a:&Type,b:&Type)->bool{a==b||matches!(a,Type::Unknown)||matches!(b,Type::Unknown)||matches!(a,Type::Reference{inner,..} if inner.as_ref()==b)}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer::Lexer,parser::Parser};
    fn check(s:&str)->Result<(),Vec<SemanticError>>{let t=Lexer::new(s).tokenize().unwrap();let p=Parser::new(t).parse().unwrap();SemanticAnalyzer::check(&p)}
    #[test]fn types(){assert!(check("fn main(){let x:i32=10 let y:u64=20u64}").is_ok());assert!(check("fn main(){let x:i32=true}").is_err());}
    #[test]fn names(){assert!(check("fn main(){println(missing)}").is_err());}
    #[test]fn mutability(){assert!(check("fn main(){let x=1 x=2}").is_err());assert!(check("fn main(){let mut x=1 x+=2 x++}").is_ok());}
    #[test]fn else_if(){assert!(check("fn main(){if true{}else if false{}else{}}").is_ok());}
}
