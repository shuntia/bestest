# APCS Tester

This is a tester program for AP computer science. It is a WIP.

Mayme I want to call it pastoxide
paratide
gradox
paragrad

## Usage:

### Setup

#### For CLI

Navigate to your directory with cd
run `apcs_tester init`
edit the configuration files

#### For GUI

Just run the program, and configure options through the GUI.

It essentially does `apcs-tester init` and display that in a GUI. If you can edit the config file directly, that is more encouraged.

### Configuration

lang: only supports `Guess` and `Java` for now.

args: command-line arguments to pass to the compiler, in list format(`[]`)

target: target directory. `apcs_tester init` automatically sets this for you.

input: List of strings that shall be passed to `stdin` for every test case

output: Expected `stdout` from program

points: Point distribution

timeout: Program timeout(in ms)

threads: Number of concurrent threads for compilation + execution. Defaults to number of cores on current system.

checker: AST or static checker. AST checker is unlikely to be implemented.

allow: Allowed dangerous program actions

format: File format of test cases(i.e. name, id, extension, num, alpha, alnum)

orderby: Order output by Name/Id

dependencies: Files to be moved into the root of the virtual environment

entry: entry point for the program(unnecessary for some languages, but currently required.)

### Allow options

FileIO: File I/O access

SysAccess: Access to libraries like `sys`

Runtime: Access to `Runtime` object

Threading: running multiple threads

Reflection: Reflecting classes

ProcessExec: Execute external programs

SystemCall: Invoke raw syscalls

Network: Networking access

Assembly: Inline assembly like `asm!()` for rust

Signal: Sending arbitrary signals to external processes

Process: Java's `ProcessBuilder`

Unsafe: `unsafe` in rust

FFI: Access to FFI

Command: Access to execute commands interpreted by `sh` or `cmd`

OsAccess: Access to OS-specific functions in python like `os.system()`

Eval: Use of python `eval`

Exec: Use of python `exec`

Import: Use of external libraries

Ctypes: Use of C types via FFI-like interfaces

Pickle: Use of pickles in python

All: Allow all


### Command-line options

```
  -v, --verbose                verbose mode

      --debug                  debug mode

      --trace                  trace mode

  -q, --quiet                  quiet mode

  -s, --silent                 silent mode

  -l, --log-level <LOG_LEVEL>  log level

      --config <CONFIG>        configuration file for tests

  -o, --output <OUTPUT>        output file or directory for results

      --dry-run                dry-run and just execute, don't input anything

  -a, --artifacts              leave artifacts

  -h, --help                   Print help
```

## Building

for CLI

```bash
git clone https://github.com/shuntia/apcs-tester
cd apcs-tester
cargo build -r
```

for GUI
```bash
git clone https://github.com/shuntia/apcs-tester
cd apcs-tester
cargo build -r --features gui
```

the binary is in `target/release`

## Installation

```bash
cargo install --git https://github.com/shuntia/apcs-tester
```

> [!NOTE]
> This program does not currently work under windows because it relies on signal sending provided by `nix`

### Contribution

Feel free to help out!

### LICENSE

   Copyright [2025] [shuntia]

   Licensed under the Apache License, Version 2.0 (the "License");
   you may not use this file except in compliance with the License.
   You may obtain a copy of the License at

     http://www.apache.org/licenses/LICENSE-2.0

   Unless required by applicable law or agreed to in writing, software
   distributed under the License is distributed on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
   See the License for the specific language governing permissions and
   limitations under the License.

