; Recontrol Lang LLVM IR
source_filename = "recontrol"

declare void @rcl_println(ptr)
declare void @rcl_print(ptr)

@.str.48656C6C6F2C205265636F6E74726F6C2100 = private unnamed_addr constant [18 x i8] c"Hello, Recontrol!\00", align 1

define void @main() {
entry:
  %l0 = alloca ptr
  store ptr getelementptr inbounds ([18 x i8], ptr @.str.48656C6C6F2C205265636F6E74726F6C2100, i64 0, i64 0), ptr %l0
  call void @rcl_println(ptr %0 = load ptr, ptr %l0)
  ret void
}


declare void @exit(i32)
define void @_start() {
entry:
  call void @main()
  call void @exit(i32 0)
  unreachable
}
