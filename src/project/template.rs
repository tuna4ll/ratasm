//! Starter files for a new project.
//!
//! The hello-world template is deliberately a *complete, working, commented*
//! program rather than a stub. It is the first thing a new user sees, and it
//! doubles as a reference for the two things that trip people up immediately:
//! the syscall argument registers, and the fact that `_start` must exit
//! explicitly because there is no runtime to return to.

/// A working `write`/`exit` program in NASM syntax for Linux x86-64.
pub const HELLO_WORLD: &str = r#"; A minimal Linux x86-64 program in NASM syntax.
;
; Assembled with:  nasm -f elf64 main.asm -o main.o
; Linked with:     ld main.o -o main

section .data
    message:      db  "Hello, world!", 10   ; 10 is the newline byte
    message_len:  equ $ - message           ; $ is "here", so this is the length

section .text
    global _start                           ; ld looks for _start by default

_start:
    ; write(1, message, message_len)
    ;
    ; Linux takes the syscall number in RAX and the arguments in
    ; RDI, RSI, RDX, R10, R8, R9 — note R10, not RCX: the syscall
    ; instruction destroys RCX and R11.
    mov     rax, 1                          ; syscall 1 is write
    mov     rdi, 1                          ; file descriptor 1 is stdout
    lea     rsi, [rel message]              ; buffer to write
    mov     rdx, message_len                ; how many bytes
    syscall

    ; exit(0)
    ;
    ; _start is not a function and has nowhere to return to, so the
    ; program must leave through the exit syscall. Falling off the end
    ; here would execute whatever bytes follow.
    mov     rax, 60                         ; syscall 60 is exit
    xor     edi, edi                        ; status 0; xor is shorter than mov
    syscall
"#;

/// A scratchpad starting point: a program that does nothing but exit cleanly.
///
/// Small on purpose. The scratchpad is for trying one instruction, so the
/// template should be the smallest thing that assembles, links and exits.
pub const SCRATCHPAD: &str = r#"; Scratchpad. Put instructions under `_start` and run.
;
; This program is assembled, linked and executed natively on this machine.

section .text
    global _start

_start:
    ; --- your instructions go here ---
    mov     rax, 1
    add     rax, 2
    ; ---------------------------------

    mov     rax, 60                         ; exit
    xor     edi, edi
    syscall
"#;

/// A program that calls `printf`, linked against the C library by `gcc`.
pub const LIBC_HELLO: &str = r#"; printf from assembly, linked with the C library.
;
; gcc links this with libc and its start-up code, which calls main like any
; C function. Arguments go in RDI, RSI, RDX, RCX, R8, R9; the stack must be
; 16-byte aligned at each call; and a variadic function such as printf wants
; in AL the number of vector registers holding arguments.

    extern printf

section .rodata
    format: db "Hello from %s! 6 * 7 = %d", 10, 0
    name:   db "libc", 0

section .text
    global main

main:
    push    rbp                     ; the call pushed 8 bytes; this realigns to 16
    lea     rdi, [rel format]       ; first argument: the format string
    lea     rsi, [rel name]         ; second: the %s
    mov     edx, 6 * 7              ; third: the %d
    xor     eax, eax                ; no vector registers used
    call    printf wrt ..plt
    xor     eax, eax                ; main returns 0
    pop     rbp
    ret

section .note.GNU-stack noalloc noexec nowrite progbits
"#;

/// The `.gitignore` written into a new project.
pub const GITIGNORE: &str = "build/\n*.o\n";

/// Renders the `.ratasm.toml` for a freshly created project.
///
/// Written by hand rather than serialised so the file a user first opens has
/// comments explaining each field, which a round-tripped struct cannot carry.
pub fn project_file(name: &str) -> String {
    project_file_linked(name, "ld", "# linker_args = [\"-n\"]")
}

/// Renders the `.ratasm.toml` for a project linked against libc by `gcc`.
pub fn libc_project_file(name: &str) -> String {
    project_file_linked(name, "gcc", "linker_args = [\"-no-pie\"]")
}

fn project_file_linked(name: &str, linker: &str, linker_args: &str) -> String {
    format!(
        r#"# ratasm project file.
# Every field here has a default; delete anything you do not need to change.

[project]
name = "{name}"
entry = "src/main.asm"
architecture = "x86_64"
syntax = "nasm"

# Additional sources assembled alongside the entry file.
# sources = ["src/util.asm"]

# Directories searched by %include.
# include_directories = ["include"]

[build]
assembler = "nasm"
assembler_args = ["-f", "elf64"]
linker = "{linker}"
{linker_args}
output_directory = "build"

[run]
args = []
# Milliseconds before the program is killed. 0 disables the limit.
timeout_ms = 5000
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::syntax::{self, TokenKind};

    #[test]
    fn the_project_file_template_parses_as_a_configuration() {
        let config = crate::project::ProjectConfig::from_toml(&project_file("demo"))
            .expect("template must parse");
        assert_eq!(config.project.name, "demo");
        config.validate().expect("template must validate");
    }

    #[test]
    fn the_libc_project_file_links_with_gcc() {
        let config = crate::project::ProjectConfig::from_toml(&libc_project_file("demo"))
            .expect("template must parse");
        assert_eq!(config.build.linker, "gcc");
        assert_eq!(config.build.linker_args, ["-no-pie"]);
    }

    #[test]
    fn the_project_name_is_embedded() {
        assert!(project_file("my-project").contains("name = \"my-project\""));
    }

    #[test]
    fn the_templates_lex_without_losing_text() {
        // A template that does not survive the lexer would render wrongly the
        // moment it is opened.
        for template in [HELLO_WORLD, SCRATCHPAD] {
            for line in template.lines() {
                let covered: usize = syntax::tokenize(line)
                    .iter()
                    .map(crate::editor::Token::len)
                    .sum();
                assert_eq!(covered, line.len(), "lexing lost text in {line:?}");
            }
        }
    }

    #[test]
    fn the_hello_world_template_defines_the_entry_point() {
        let has_start = HELLO_WORLD
            .lines()
            .any(|line| syntax::label_definition(line).as_deref() == Some("_start"));
        assert!(has_start, "_start must be defined");
        assert!(
            HELLO_WORLD.contains("global _start"),
            "_start must be exported"
        );
    }

    #[test]
    fn the_hello_world_template_exits_through_a_syscall() {
        // Without this the program runs off the end of the code.
        assert!(HELLO_WORLD.contains("mov     rax, 60"));
        assert!(HELLO_WORLD.contains("syscall"));
    }

    #[test]
    fn the_hello_world_template_uses_both_required_sections() {
        assert!(HELLO_WORLD.contains("section .data"));
        assert!(HELLO_WORLD.contains("section .text"));
    }

    #[test]
    fn the_scratchpad_template_is_a_complete_program() {
        assert!(SCRATCHPAD.contains("global _start"));
        assert!(SCRATCHPAD.contains("mov     rax, 60"));
        let has_start = SCRATCHPAD
            .lines()
            .any(|line| syntax::label_definition(line).as_deref() == Some("_start"));
        assert!(has_start);
    }

    #[test]
    fn the_scratchpad_states_that_code_runs_natively() {
        // The tool must not imply the scratchpad is a sandbox.
        assert!(SCRATCHPAD.contains("natively on this machine"));
    }

    #[test]
    fn templates_use_only_recognised_instructions() {
        for template in [HELLO_WORLD, SCRATCHPAD] {
            for line in template.lines() {
                for token in syntax::tokenize(line) {
                    if token.kind == TokenKind::Identifier {
                        let text = token.text(line);
                        // Identifiers left over must be symbols the template
                        // itself defines, not misspelled mnemonics.
                        assert!(
                            template.contains(&format!("{text}:"))
                                || template.contains(&format!("{text}  "))
                                || text.starts_with('.')
                                || text == "message_len",
                            "unexpected identifier {text:?} in template"
                        );
                    }
                }
            }
        }
    }
}
