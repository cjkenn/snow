# kolga

kolga is an educational compiler (always WIP) with the following goals:

1. Learn the LLVM API by using it to generate LLVM IR and machine code.
2. Learn about type systems by building a type checker, type inference, and more complex ADT's.
3. Support object oriented composability.
4. Eventually (maybe), build some light concurrency primitives like coroutines.

Right now, kolga features basic language syntax:
1. Functions (with recursion)
2. Classes
3. Basic control flow: if/else/else if, while loops, for loops
4. A few different types: 64-bit numbers, ASCII strings, bool, and void

Some compiler features so far:
1. Lexing and parsing into an AST
2. Type checking, including for ADT's
3. LLVM IR codegen directly from AST (no additional IR) and optimization manager
 
### Requirements

1. [rust](https://rust-lang.org)
2. [llvm (supports 6.0)](https://llvm.org)

### Running
```sh
cargo run [filename]
```

### Testing
```sh
cargo test
```

### Project Layout
`libkolgac` contains code for lexing and parsing, as well as appropriate token and AST data structures.

`libtypes` contains type checking/inference

`libllvm_codegen` handles generating the llvm ir from the ast

`liberrors` is a smaller package containing some convenienve functions for error handling (pretty bare bones right now)

The `src` directory contains the file `kolga.rs`, which is the main entry point into the compiler. 

### Some Small Examples

```
fn basicFunc(x~num, y~num)~num {
  let z~num = 10;
  return x + y + z;
}

fn double(x~num)~num {
  return x * 2;
}

fn negate(y~bool)~bool {
  return !y;
}
```

```
class mClass {
  let z~num;
  let y~num = 10;
  
  fn nop()~void {
    return;
  }
  
  fn triple()~num {
    return y * 3;
  }
}
```

