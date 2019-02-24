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
3. Basic type inference for variable assignments. Functions still require type annotations.
4. LLVM IR codegen directly from AST (no additional IR) and optimization manager
 
### Requirements

1. [rust](https://rust-lang.org)
2. [llvm (supports 6.0)](https://llvm.org)

### Running
```sh
cargo run [filename]
```

### Testing
```sh
cargo test -- --nocapture
```

### Some Examples
```
fn basicFunc(x~num)~num {
  let inferMe ~= x + 10;
  let dontInferMe~num = x - 1;
  return inferMe;
}

let plsInferMe ~= basicFunc(1);
```

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
  let y~num;
  
  fn nop()~void {
    return;
  }
  
  fn triple()~num {
    return self.y * 3;
  }
}

let instance~mClass {
  z = 1,
  y = 10,
};

instance.nop();
let tripled ~= instance.triple(); // 30
```

### Project Layout
`kolgac` contains code for lexing and parsing, as well as appropriate token and AST data structures. This is the core compiler.

`ty` contains type checking/inference

`gen` handles generating the llvm ir from the ast

`error` contains error handling/emitting functions, are well as error types for each stage in the compiler

The `src` directory contains the file `kolga.rs`, which is the main entry point into the compiler.

### A note on command line parsing
This project does not follow the standard rust docs on command line parsing, and really just hacks in its own implementation of a rudimentary argument parser. Why? `structopt` was having some problems working properly on my machine, and was messing up my terminal when printing some messages.

