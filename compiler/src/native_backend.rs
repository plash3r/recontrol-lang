use crate::ast::{BinaryOp, Literal, UnaryOp};
use crate::hir::{BUILTIN_PRINT_ID, BUILTIN_PRINTLN_ID};
use crate::mir::{MirFunction, MirProgram, MirStatement, Operand, Place, Rvalue, Terminator};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodegenError { pub function: String, pub message: String }

pub struct NativeBackend;

#[derive(Default)]
struct E {
    b: Vec<u8>,
    labels: HashMap<usize, usize>,
    jumps: Vec<(usize, usize, bool)>,
    calls: Vec<(usize, String)>,
    strs: Vec<(String, Vec<u8>)>,
    refs: Vec<(usize, String)>,
}
impl E {
    fn p(&self)->usize{self.b.len()}
    fn w(&mut self,x:&[u8]){self.b.extend_from_slice(x)}
    fn label(&mut self,id:usize){self.labels.insert(id,self.p());}
    fn imm(&mut self,v:i64){self.w(&[0x48,0xb8]);self.b.extend_from_slice(&(v as u64).to_le_bytes());}
    fn rr(&mut self,d:u8,s:u8){self.w(&[0x48,0x89,0xc0|(s<<3)|d]);}
    fn ld(&mut self,o:i32){self.w(&[0x48,0x8b,0x85]);self.b.extend_from_slice(&o.to_le_bytes());}
    fn st(&mut self,o:i32){self.w(&[0x48,0x89,0x85]);self.b.extend_from_slice(&o.to_le_bytes());}
    fn push(&mut self,r:u8){self.w(&[0x50+r]);}
    fn pop(&mut self,r:u8){self.w(&[0x58+r]);}
    fn pro(&mut self,n:u32){self.w(&[0x55,0x48,0x89,0xe5]);if n>0{self.w(&[0x48,0x81,0xec]);self.b.extend_from_slice(&n.to_le_bytes());}}
    fn epi(&mut self){self.w(&[0xc9,0xc3]);}
    fn jmp(&mut self,id:usize){let p=self.p();self.w(&[0xe9,0,0,0,0]);self.jumps.push((p,id,false));}
    fn jcc(&mut self,id:usize){let p=self.p();self.w(&[0x0f,0x85,0,0,0,0]);self.jumps.push((p,id,true));}
    fn call(&mut self,n:String){let p=self.p();self.w(&[0xe8,0,0,0,0]);self.calls.push((p,n));}
    fn strref(&mut self,n:String){let p=self.p();self.w(&[0x48,0x8d,0x35,0,0,0,0]);self.refs.push((p,n));}
    fn intern(&mut self,x:Vec<u8>)->String{if let Some((n,_))=self.strs.iter().find(|(_,b)|*b==x){return n.clone()}let n=format!(".s{}",self.strs.len());self.strs.push((n.clone(),x));n}
    fn patch(mut self, funcs:&HashMap<String,usize>, base:usize, ro:usize)->Result<Vec<u8>,String>{
        for (p,id,long) in &self.jumps{let t=*self.labels.get(id).ok_or_else(||format!("missing block {id}"))?;let next=p+if *long{6}else{5};let d=t as isize-next as isize;let q=p+if *long{2}else{1};self.b[q..q+4].copy_from_slice(&(d as i32).to_le_bytes());}
        for (p,n) in &self.calls{let t=*funcs.get(n).ok_or_else(||format!("missing function {n}"))?;let d=(base+t) as isize-(base+p+5) as isize;self.b[p+1..p+5].copy_from_slice(&(d as i32).to_le_bytes());}
        for (p,n) in &self.refs{let mut q=ro;let mut found=None;for (x,b) in &self.strs{if x==n{found=Some(q);break}q+=b.len()}let t=found.ok_or_else(||format!("missing string {n}"))?;let d=t as isize-(base+p+7) as isize;self.b[p+3..p+7].copy_from_slice(&(d as i32).to_le_bytes());}
        Ok(self.b)
    }
}

impl NativeBackend {
    pub fn emit(p:&MirProgram)->Result<Vec<u8>,Vec<CodegenError>>{
        #[cfg(all(target_os="linux",target_arch="x86_64"))] { return emit_linux(p); }
        #[cfg(not(all(target_os="linux",target_arch="x86_64")))] {
            let _=p;
            Err(vec![CodegenError{function:"<module>".into(),message:"standalone native backend currently supports Linux x86-64".into()}])
        }
    }
}

#[cfg(all(target_os="linux",target_arch="x86_64"))]
fn emit_linux(p:&MirProgram)->Result<Vec<u8>,Vec<CodegenError>>{
    let mut raw=Vec::new();let mut es=Vec::new();let mut errs=Vec::new();
    for f in &p.functions{
        let mut e=E::default();let frame=(((f.locals.len().max(1))*8+15)/16*16) as u32;e.pro(frame);
        let ar=[7u8,6,2,1,8,9];
        for (i,l) in f.locals.iter().take(f.param_count).enumerate(){if i>=6{errs.push(er(f,"more than 6 parameters unsupported"));break}e.rr(10+i as u8,ar[i]);e.st(-8*(l.id as i32+1));}
        for b in &f.blocks{e.label(b.id);if let Err(x)=block(&mut e,p,f,b){errs.push(er(f,&x));}}
        raw.extend_from_slice(&e.b);es.push((f.name.clone(),e,raw.len())); 
    }
    if !errs.is_empty(){return Err(errs)}
    let stub=14usize;let base=120usize;let mut funcs=HashMap::new();let mut prev=0usize;
    for (n,_,end) in &es{funcs.insert(n.clone(),stub+prev);prev=*end;}
    let mut strings=Vec::<(String,Vec<u8>)>::new();for (_,e,_) in &es{for x in &e.strs{if !strings.iter().any(|(n,_):&(String,Vec<u8>)|n==&x.0){strings.push(x.clone())}}}
    let ro=base+stub+raw.len();let mut rod=Vec::new();for (_,b) in &strings{rod.extend_from_slice(b)}
    let mut code=entry();let mut off=0usize;
    for (_,e,end) in es{let _=end;let n=e.patch(&funcs,base,ro).unwrap();code.extend_from_slice(&n);off+=n.len();let _=off;}
    let main=*funcs.get("main").unwrap_or(&stub);let d=(base+main) as isize-(base+5) as isize;code[1..5].copy_from_slice(&(d as i32).to_le_bytes());code.extend_from_slice(&rod);
    Ok(elf(code,base))
}
#[cfg(all(target_os="linux",target_arch="x86_64"))]
fn er(f:&MirFunction,m:&str)->CodegenError{CodegenError{function:f.name.clone(),message:m.into()}}
#[cfg(all(target_os="linux",target_arch="x86_64"))]
fn block(e:&mut E,p:&MirProgram,f:&MirFunction,b:&crate::mir::BasicBlock)->Result<(),String>{
    for s in &b.statements{match s{
        MirStatement::StorageLive(_)|MirStatement::StorageDead(_)=>{},
        MirStatement::Assign{place,rvalue}=>{let Place::Local(id)=place else{return Err("field/index assignment unsupported".into())};rv(e,p,f,rvalue)?;e.st(-8*(*id as i32+1));},
        MirStatement::Evaluate(v)=>{rv(e,p,f,v)?;}
    }}
    match &b.terminator{
        Terminator::Goto(x)=>e.jmp(*x),
        Terminator::SwitchBool{condition,then_block,else_block}=>{op(e,f,condition)?;e.w(&[0x48,0x85,0xc0]);e.jcc(*then_block);e.jmp(*else_block)},
        Terminator::Return(v)=>{if let Some(v)=v{rv(e,p,f,v)?}else{e.imm(0)}e.epi()},
        Terminator::Unreachable=>return Err("unreachable terminator".into())
    }Ok(())
}
#[cfg(all(target_os="linux",target_arch="x86_64"))]
fn rv(e:&mut E,p:&MirProgram,f:&MirFunction,v:&Rvalue)->Result<(),String>{
    match v{
        Rvalue::Use(o)=>op(e,f,o),
        Rvalue::Unary{op:u,operand}=>{op(e,f,operand)?;match u{UnaryOp::Plus=>{},UnaryOp::Minus=>e.w(&[0x48,0xf7,0xd8]),UnaryOp::Not=>e.w(&[0x48,0x83,0xf0,1]),_=>return Err("references unsupported".into())}Ok(())},
        Rvalue::Binary{left,op:u,right}=>{op(e,f,left)?;e.push(&[0]);op(e,f,right)?;e.rr(3,0);e.pop(0);match u{
            BinaryOp::Add=>e.w(&[0x48,1,0xd8]),BinaryOp::Subtract=>e.w(&[0x48,0x29,0xd8]),BinaryOp::Multiply=>e.w(&[0x48,0x0f,0xaf,0xc3]),
            BinaryOp::Divide=>{e.rr(3,0);e.w(&[0x48,0x99,0x48,0xf7,0xfb])},BinaryOp::Modulo=>{e.rr(3,0);e.w(&[0x48,0x99,0x48,0xf7,0xfb]);e.rr(0,2)},
            BinaryOp::And=>e.w(&[0x48,0x21,0xd8]),BinaryOp::Or=>e.w(&[0x48,9,0xd8]),
            BinaryOp::Equal=>cc(e,4),BinaryOp::NotEqual=>cc(e,5),BinaryOp::Less=>cc(e,12),BinaryOp::LessEqual=>cc(e,14),BinaryOp::Greater=>cc(e,15),BinaryOp::GreaterEqual=>cc(e,13)
        }Ok(())},
        Rvalue::Call{callee,args}=>{let crate::mir::Operand::Function(id)=callee else{return Err("indirect calls unsupported".into())};if *id==BUILTIN_PRINT_ID||*id==BUILTIN_PRINTLN_ID{return print(e,args,*id==BUILTIN_PRINTLN_ID)}let n=p.functions.get(*id).ok_or("bad function id")?.name.clone();if args.len()>6{return Err("more than 6 call arguments unsupported".into())}let r=[7u8,6,2,1,8,9];for (i,a) in args.iter().enumerate().rev(){op(e,f,a)?;e.rr(r[i],0)}e.call(n);Ok(())},
        _=>Err("this Rvalue is not supported by standalone backend yet".into())
    }
}
#[cfg(all(target_os="linux",target_arch="x86_64"))]
fn cc(e:&mut E,c:u8){e.w(&[0x48,0x39,0xd8,0x0f,0x90|c,0xc0,0x48,0x0f,0xb6,0xc0]);}
#[cfg(all(target_os="linux",target_arch="x86_64"))]
fn op(e:&mut E,f:&MirFunction,o:&Operand)->Result<(),String>{match o{
    Operand::Constant(Literal::Number(n))=>{let (v,s)=num(n);if s.starts_with('f'){return Err("floating point unsupported".into())}e.imm(v.parse().map_err(|_|"invalid integer literal")?);Ok(())},
    Operand::Constant(Literal::Bool(x))=>{e.imm(if *x{1}else{0});Ok(())},
    Operand::Constant(Literal::String(s))=>{let n=e.intern(s.as_bytes().iter().copied().chain([0]).collect());e.strref(n);Ok(())},
    Operand::Copy(Place::Local(id))|Operand::Move(Place::Local(id))=>{e.ld(-8*(*id as i32+1));Ok(())},
    _=>{let _=f;Err("operand unsupported".into())}
}}
#[cfg(all(target_os="linux",target_arch="x86_64"))]
fn print(e:&mut E,args:&[Operand],nl:bool)->Result<(),String>{if args.len()!=1{return Err("print/println require one argument".into())}let Operand::Constant(Literal::String(s))=&args[0]else{return Err("print/println currently require a string literal".into())};let b=s.as_bytes().to_vec();let n=e.intern(b.clone());e.strref(n);e.rr(6,0);e.imm(b.len() as i64);e.rr(2,0);e.imm(1);e.rr(7,0);e.w(&[0x0f,5]);if nl{let n=e.intern(vec![10]);e.strref(n);e.rr(6,0);e.imm(1);e.rr(2,0);e.imm(1);e.rr(7,0);e.w(&[0x0f,5]);}Ok(())}
fn num(n:&str)->(&str,&str){let mut i=n.len();while i>0&&n.as_bytes()[i-1].is_ascii_alphabetic(){i-=1}(&n[..i],&n[i..])}
#[cfg(all(target_os="linux",target_arch="x86_64"))]
fn entry()->Vec<u8>{vec![0xe8,0,0,0,0,0x89,0xc7,0xb8,60,0,0,0,0x0f,5]}
#[cfg(all(target_os="linux",target_arch="x86_64"))]
fn elf(code:Vec<u8>,base:usize)->Vec<u8>{let va=0x400000u64;let size=120+code.len();let mut o=Vec::with_capacity(size);o.extend_from_slice(&[0x7f,b'E',b'L',b'F',2,1,1,0,0,0,0,0,0,0,0,0]);o.extend_from_slice(&2u16.to_le_bytes());o.extend_from_slice(&0x3eu16.to_le_bytes());o.extend_from_slice(&1u32.to_le_bytes());o.extend_from_slice(&(va+base as u64).to_le_bytes());o.extend_from_slice(&64u64.to_le_bytes());o.extend_from_slice(&0u64.to_le_bytes());o.extend_from_slice(&0u32.to_le_bytes());o.extend_from_slice(&64u16.to_le_bytes());o.extend_from_slice(&56u16.to_le_bytes());o.extend_from_slice(&1u16.to_le_bytes());o.extend_from_slice(&0u16.to_le_bytes());o.extend_from_slice(&0u16.to_le_bytes());o.extend_from_slice(&0u16.to_le_bytes());o.extend_from_slice(&1u32.to_le_bytes());o.extend_from_slice(&7u32.to_le_bytes());o.extend_from_slice(&0u64.to_le_bytes());o.extend_from_slice(&va.to_le_bytes());o.extend_from_slice(&va.to_le_bytes());o.extend_from_slice(&(size as u64).to_le_bytes());o.extend_from_slice(&(size as u64).to_le_bytes());o.extend_from_slice(&0x1000u64.to_le_bytes());o.extend_from_slice(&code);o}
