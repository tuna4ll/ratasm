//! Guided material: short lessons and questions with worked answers.
//!
//! # The answers are checked against the processor
//!
//! Every question states the register value the instruction produces. Those
//! numbers are not taken on trust: a test runs each question through the
//! [scratchpad](crate::scratchpad), which assembles and executes it for real,
//! and fails if the stated answer differs from what the CPU actually did.
//!
//! That matters because the questions cover exactly the cases people get wrong
//! — arithmetic versus logical shift, signed versus unsigned comparison, what
//! `syscall` destroys. Material that teaches those confidently and incorrectly
//! would be worse than none.

use crate::instruction::Flag;

/// A short explanation of one idea.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lesson {
    /// Stable identifier.
    pub id: &'static str,
    /// The heading.
    pub title: &'static str,
    /// Paragraphs, in reading order.
    pub body: &'static [&'static str],
}

/// A question with a checkable answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    /// Stable identifier.
    pub id: &'static str,
    /// What the reader is asked.
    pub prompt: &'static str,
    /// Register values the question starts from.
    pub setup: &'static [(&'static str, u64)],
    /// The instruction under discussion.
    pub instruction: &'static str,
    /// The register the answer is about.
    pub register: &'static str,
    /// The value that register ends up holding.
    pub answer: u64,
    /// Why, worked through, shown after an answer is given.
    pub explanation: &'static str,
    /// Flags the reader should expect to see set afterwards.
    pub flags_set: &'static [Flag],
}

impl Question {
    /// Whether a typed answer is correct.
    ///
    /// Accepts decimal, hexadecimal and negative decimal, because a reader
    /// thinking about `sar` naturally writes `-1` while one thinking about
    /// bits writes `0xffffffffffffffff`, and both are right.
    pub fn is_correct(&self, typed: &str) -> bool {
        parse_answer(typed).is_some_and(|value| value == self.answer)
    }

    /// The answer as the reader is most likely to recognise it.
    pub fn answer_text(&self) -> String {
        let signed = self.answer as i64;
        if signed < 0 {
            format!("{:#x} ({signed})", self.answer)
        } else {
            format!("{:#x} ({})", self.answer, self.answer)
        }
    }

    /// The question as a runnable scratchpad.
    ///
    /// Used by the tests to check the stated answer against the processor,
    /// and by the interface to let the reader try it themselves.
    pub fn to_scratchpad(&self) -> crate::scratchpad::Scratchpad {
        let mut pad = crate::scratchpad::Scratchpad::new();
        for (name, value) in self.setup {
            // Every name here is checked by a test, so a failure would be a
            // bug in the material rather than in user input.
            let _ = pad.set(name, *value);
        }
        pad.snippet = self.instruction.to_owned();
        pad
    }
}

/// Parses a typed answer in any reasonable notation.
pub fn parse_answer(text: &str) -> Option<u64> {
    let text = text.trim().replace('_', "");
    if text.is_empty() {
        return None;
    }

    let lowered = text.to_ascii_lowercase();
    if let Some(hex) = lowered.strip_prefix("0x") {
        return u64::from_str_radix(hex, 16).ok();
    }
    if let Some(binary) = lowered.strip_prefix("0b") {
        return u64::from_str_radix(binary, 2).ok();
    }
    if let Some(negative) = text.strip_prefix('-') {
        // A negative answer is the two's complement pattern the register
        // actually holds.
        return negative.parse::<i64>().ok().map(|value| (-value) as u64);
    }
    text.parse::<u64>().ok()
}

/// The lessons, in reading order.
pub const LESSONS: &[Lesson] = &[
    Lesson {
        id: "registers",
        title: "The registers and what they are for",
        body: &[
            "x86-64 has sixteen general-purpose registers. They are interchangeable to the \
             processor, but software agrees on what each is for, and ignoring that agreement is \
             how programs break in ways that are hard to see.",
            "RAX carries a function's return value and a syscall's number. RDI, RSI, RDX, RCX, R8 \
             and R9 carry the first six integer arguments to a function, in that order.",
            "RBX, RBP, RSP and R12 through R15 are callee-saved: if you use one, you must put it \
             back before returning. The rest you may destroy freely.",
            "Every register has narrower views over the same bits. RAX is 64 bits, EAX is its low \
             32, AX its low 16, AL its low 8, and AH is bits 8 to 15 — not a second name for AL.",
            "Writing to a 32-bit register zeroes the upper 32 bits of its 64-bit parent. Writing \
             to a 16- or 8-bit one does not. That asymmetry surprises everyone once.",
        ],
    },
    Lesson {
        id: "syscalls",
        title: "Asking the kernel to do something",
        body: &[
            "A syscall is how a program asks the kernel for anything it cannot do itself: reading \
             a file, writing to the terminal, exiting.",
            "Put the call number in RAX and the arguments in RDI, RSI, RDX, R10, R8, R9. Then \
             execute `syscall`. The result comes back in RAX.",
            "The fourth argument goes in R10, not RCX. This is the single most common mistake, \
             and the reason is mechanical: the `syscall` instruction itself overwrites RCX with \
             the return address and R11 with the flags, so RCX cannot carry an argument.",
            "Errors come back as small negative numbers in RAX rather than through a separate \
             channel. A return of -2 means ENOENT.",
            "Press Ctrl+K to search the whole table by name or number.",
        ],
    },
    Lesson {
        id: "stack",
        title: "How the stack works",
        body: &[
            "The stack grows downwards. `push` subtracts 8 from RSP and writes at the new \
             address; `pop` reads and adds 8 back.",
            "RSP always points at the most recently pushed value. RBP, when used, marks the base \
             of the current frame so local values can be reached at fixed offsets.",
            "`call` pushes the return address and jumps. `ret` pops whatever is at RSP and jumps \
             to it — it does not verify that the value is a return address. An unbalanced stack \
             therefore sends execution somewhere arbitrary, and the crash appears far from the \
             mistake.",
            "The ABI requires RSP to be 16-byte aligned at the point a `call` executes. Forgetting \
             this breaks library functions that use aligned vector loads, usually inside printf.",
        ],
    },
    Lesson {
        id: "flags",
        title: "Flags and conditional jumps",
        body: &[
            "Arithmetic and logic instructions leave a record of their result in the flags. `cmp` \
             and `test` exist only to set flags: they compute a result and discard it.",
            "ZF is set when the result was zero, SF when it was negative, CF on unsigned overflow \
             and OF on signed overflow.",
            "Signed and unsigned comparisons use different flags and different jumps. After `cmp \
             a, b`, use `jg` when the values are signed and `ja` when they are unsigned. They are \
             not interchangeable, and mixing them up produces code that works until it meets a \
             value above 0x7fffffffffffffff.",
            "The flag panel evaluates every condition against the flags as they stand and lists \
             the jumps that would be taken right now.",
        ],
    },
    Lesson {
        id: "calling-convention",
        title: "Calling a function, and being called",
        body: &[
            "To call: put the arguments in RDI, RSI, RDX, RCX, R8, R9, make sure RSP is 16-byte \
             aligned, and `call`. Assume RAX, RCX, RDX, RSI, RDI and R8 through R11 are destroyed \
             when it returns.",
            "To be callable: preserve RBX, RBP and R12 through R15 if you touch them, leave the \
             direction flag clear, and return your result in RAX.",
            "A function that returns without restoring a callee-saved register corrupts its \
             caller, and the damage shows up later in unrelated code.",
        ],
    },
    Lesson {
        id: "memory",
        title: "Reading and writing memory",
        body: &[
            "Square brackets mean memory. `mov rax, [rbx]` loads the eight bytes at the address \
             in RBX; `mov rax, rbx` copies the register itself.",
            "An address can be built from a base, an index scaled by 1, 2, 4 or 8, and a \
             displacement: `[rbx + rcx*8 + 16]` is element RCX of an array of quadwords that \
             starts 16 bytes past RBX.",
            "When nothing else fixes the size, say it: `mov byte [rdi], 0` writes one byte, \
             `mov qword [rdi], 0` writes eight. NASM refuses to guess.",
            "In a position-independent program, reach your own data with `[rel label]`. It is \
             addressed relative to RIP, so the program works wherever it is loaded.",
            "x86-64 is little-endian: the least significant byte is stored at the lowest \
             address. The Memory panel (Ctrl+G) shows the bytes as they lie.",
        ],
    },
    Lesson {
        id: "loops",
        title: "Loops",
        body: &[
            "A loop is a counter, a body and a conditional jump back. The usual shape counts \
             down: `mov rcx, 10`, then the body, then `dec rcx` and `jnz` back to the top.",
            "Counting down needs no `cmp`, because `dec` sets ZF when it reaches zero. Counting \
             up needs an explicit `cmp rcx, limit` before the jump.",
            "The `loop` instruction does `dec rcx` and `jnz` in one, but it is slower on modern \
             processors and ties you to RCX. Compilers do not emit it.",
            "Remember that `syscall` and most library calls destroy RCX. A counter that must \
             survive a call belongs in a callee-saved register such as RBX or R12.",
        ],
    },
    Lesson {
        id: "multiply-divide",
        title: "Multiplying and dividing",
        body: &[
            "`imul rax, rbx` is the everyday multiply: signed, two operands, the result truncated \
             to 64 bits. It is what you want for indexes and ordinary arithmetic.",
            "The one-operand forms `mul rbx` and `imul rbx` multiply by RAX and leave a 128-bit \
             result split across RDX (high half) and RAX (low half). They overwrite RDX even when \
             you did not ask for it.",
            "`div rbx` divides the 128-bit value RDX:RAX by RBX, leaving the quotient in RAX and \
             the remainder in RDX. Clear RDX first with `xor edx, edx`, or the quotient is wrong \
             or the division faults.",
            "For signed division use `idiv`, and sign-extend RAX into RDX with `cqo` first.",
        ],
    },
    Lesson {
        id: "bits",
        title: "Bits, masks and tests",
        body: &[
            "`and` keeps bits, `or` sets them, `xor` flips them. With a constant, each is a mask: \
             `and eax, 0xff` keeps the low byte, `or eax, 0x20` sets bit 5.",
            "`test` is an `and` that only sets flags. `test rax, rax` followed by `jz` asks \
             whether RAX is zero; `test al, 1` followed by `jnz` asks whether it is odd.",
            "Shifts multiply and divide by powers of two. `shl` and `shr` treat the value as \
             unsigned; `sar` keeps the sign. Rotates (`rol`, `ror`) move the bits that fall off \
             one end back in at the other.",
        ],
    },
    Lesson {
        id: "strings",
        title: "String instructions",
        body: &[
            "`movsb`, `stosb`, `lodsb`, `cmpsb` and `scasb` work through memory with RSI as the \
             source and RDI as the destination, stepping both after each byte.",
            "A `rep` prefix repeats the instruction RCX times. `rep movsb` copies RCX bytes from \
             [RSI] to [RDI]; `rep stosb` fills RCX bytes at [RDI] with AL.",
            "The direction flag decides whether they step forwards or backwards. The ABI \
             guarantees it is clear on entry to a function; if you set it with `std`, clear it \
             again with `cld` before returning.",
        ],
    },
    Lesson {
        id: "libc",
        title: "Calling the C library",
        body: &[
            "A program that uses libc starts at `main`, not `_start`: the C start-up code runs \
             first and calls `main` like any function. `ratasm new --libc` creates one, linked \
             by gcc.",
            "Declare what you use with `extern printf` and call it as `call printf wrt ..plt`, \
             which goes through the procedure linkage table and works however the program is \
             linked.",
            "The calling convention applies in full. Arguments go in RDI, RSI, RDX, RCX, R8, R9; \
             RSP must be 16-byte aligned at the `call`; and a variadic function such as printf \
             reads AL for the number of vector registers holding arguments, so clear EAX when \
             there are none.",
            "Return from `main` with your exit status in EAX. The C library flushes its output \
             buffers on the way out, which a raw `exit` syscall would skip.",
        ],
    },
];

/// The questions, in reading order.
///
/// Each answer is verified against the real processor by a test in this
/// module, so nothing here can quietly teach something false.
pub const QUESTIONS: &[Question] = &[
    Question {
        id: "add",
        prompt: "RAX holds 1 and RBX holds 2. What is in RAX after `add rax, rbx`?",
        setup: &[("rax", 1), ("rbx", 2)],
        instruction: "add rax, rbx",
        register: "rax",
        answer: 3,
        explanation: "`add` writes the sum into its first operand, so RAX becomes 1 + 2. RBX is \
                      left alone; only the destination is written.",
        flags_set: &[],
    },
    Question {
        id: "sub-zero",
        prompt: "RAX holds 7. What is in RAX after `sub rax, rax`?",
        setup: &[("rax", 7)],
        instruction: "sub rax, rax",
        register: "rax",
        answer: 0,
        explanation: "Any value minus itself is zero, whatever it was. Because the result is \
                      zero, ZF is set — which is what makes this a common way to both clear a \
                      register and set up a following `jz`.",
        flags_set: &[Flag::Zero],
    },
    Question {
        id: "xor-clear",
        prompt: "RAX holds 0xdeadbeef. What is in RAX after `xor rax, rax`?",
        setup: &[("rax", 0xdead_beef)],
        instruction: "xor rax, rax",
        register: "rax",
        answer: 0,
        explanation: "A value exclusive-ORed with itself is zero. This is the idiomatic way to \
                      clear a register: it is shorter than `mov rax, 0` and the processor \
                      recognises it as breaking the dependency on the old value.",
        flags_set: &[Flag::Zero],
    },
    Question {
        id: "shl",
        prompt: "RAX holds 5. What is in RAX after `shl rax, 2`?",
        setup: &[("rax", 5)],
        instruction: "shl rax, 2",
        register: "rax",
        answer: 20,
        explanation: "Shifting left by n multiplies by 2^n, so 5 becomes 5 x 4 = 20.",
        flags_set: &[],
    },
    Question {
        id: "sar-negative",
        prompt: "RAX holds -8 (0xfffffffffffffff8). What is in RAX after `sar rax, 1`?",
        setup: &[("rax", 0xffff_ffff_ffff_fff8)],
        instruction: "sar rax, 1",
        register: "rax",
        answer: 0xffff_ffff_ffff_fffc,
        explanation: "`sar` is the arithmetic shift: it copies the sign bit into the vacated \
                      position, so -8 becomes -4. `shr` would have shifted in a zero and turned \
                      the same bits into a huge positive number. This is why signed division by \
                      a power of two uses `sar`.",
        flags_set: &[],
    },
    Question {
        id: "shr-negative",
        prompt: "RAX holds -8 (0xfffffffffffffff8). What is in RAX after `shr rax, 1`?",
        setup: &[("rax", 0xffff_ffff_ffff_fff8)],
        instruction: "shr rax, 1",
        register: "rax",
        answer: 0x7fff_ffff_ffff_fffc,
        explanation: "`shr` shifts in a zero, so the sign bit is lost and the value becomes a \
                      very large positive number. Compare it with `sar` on the same input: the \
                      two differ only for negative values, which is exactly when it matters.",
        flags_set: &[],
    },
    Question {
        id: "movzx",
        prompt: "RAX holds 0xffffffffffffffff. What is in RAX after `mov eax, 1`?",
        setup: &[("rax", 0xffff_ffff_ffff_ffff)],
        instruction: "mov eax, 1",
        register: "rax",
        answer: 1,
        explanation: "Writing to a 32-bit register zeroes the upper 32 bits of its 64-bit parent, \
                      so the whole of RAX becomes 1 rather than only its low half. Writing to AX \
                      or AL would not do this — they leave the surrounding bits untouched.",
        flags_set: &[],
    },
    Question {
        id: "mov-al",
        prompt: "RAX holds 0xffffffffffffffff. What is in RAX after `mov al, 1`?",
        setup: &[("rax", 0xffff_ffff_ffff_ffff)],
        instruction: "mov al, 1",
        register: "rax",
        answer: 0xffff_ffff_ffff_ff01,
        explanation: "An 8-bit write touches only the low byte, so the rest of RAX survives. \
                      This is the other half of the rule: 32-bit writes clear the upper half, \
                      8- and 16-bit writes do not.",
        flags_set: &[],
    },
    Question {
        id: "inc-carry",
        prompt: "RAX holds 0xffffffffffffffff. What is in RAX after `inc rax`?",
        setup: &[("rax", 0xffff_ffff_ffff_ffff)],
        instruction: "inc rax",
        register: "rax",
        answer: 0,
        explanation: "The value wraps around to zero. `inc` deliberately leaves CF unchanged — \
                      that is the whole difference between it and `add rax, 1`, which would have \
                      set CF to record the carry out.",
        flags_set: &[Flag::Zero],
    },
    Question {
        id: "lea",
        prompt: "RAX holds 10 and RBX holds 3. What is in RAX after `lea rax, [rax+rbx*2]`?",
        setup: &[("rax", 10), ("rbx", 3)],
        instruction: "lea rax, [rax+rbx*2]",
        register: "rax",
        answer: 16,
        explanation: "Despite the brackets, `lea` reads no memory: it computes the address and \
                      stores it. So RAX becomes 10 + 3 x 2 = 16. It is often used purely as \
                      arithmetic, because it can add and multiply in one instruction without \
                      touching the flags.",
        flags_set: &[],
    },
    Question {
        id: "neg",
        prompt: "RAX holds 5. What is in RAX after `neg rax`?",
        setup: &[("rax", 5)],
        instruction: "neg rax",
        register: "rax",
        answer: 0xffff_ffff_ffff_fffb,
        explanation: "`neg` replaces the value with its two's complement negation, so 5 becomes \
                      -5, whose bit pattern is 0xfffffffffffffffb. CF ends up set because the \
                      operand was not zero.",
        flags_set: &[Flag::Carry],
    },
    Question {
        id: "and-mask",
        prompt: "RAX holds 0x1234. What is in RAX after `and rax, 0xff`?",
        setup: &[("rax", 0x1234)],
        instruction: "and rax, 0xff",
        register: "rax",
        answer: 0x34,
        explanation: "Bitwise AND with 0xff keeps only the low byte, which is how you mask a \
                      value down to a byte. `and` also clears CF and OF unconditionally.",
        flags_set: &[],
    },
    Question {
        id: "imul",
        prompt: "RAX holds -3 and RBX holds 7. What is in RAX after `imul rax, rbx`?",
        setup: &[("rax", 0xffff_ffff_ffff_fffd), ("rbx", 7)],
        instruction: "imul rax, rbx",
        register: "rax",
        answer: 0xffff_ffff_ffff_ffeb,
        explanation: "The two-operand `imul` is a signed multiply truncated to 64 bits, so the \
                      result is -21, stored as 0xffffffffffffffeb. It fits, so CF and OF stay \
                      clear.",
        flags_set: &[],
    },
    Question {
        id: "mul-high",
        prompt: "RAX holds 0xffffffffffffffff and RBX holds 2. What is in RDX after `mul rbx`?",
        setup: &[("rax", 0xffff_ffff_ffff_ffff), ("rbx", 2), ("rdx", 0)],
        instruction: "mul rbx",
        register: "rdx",
        answer: 1,
        explanation: "The one-operand `mul` produces a 128-bit product in RDX:RAX. \
                      (2^64 - 1) x 2 = 2^65 - 2, whose high half is 1. CF and OF are set because \
                      the result did not fit in RAX alone.",
        flags_set: &[Flag::Carry, Flag::Overflow],
    },
    Question {
        id: "div-remainder",
        prompt: "RAX holds 17, RDX holds 0 and RBX holds 5. What is in RDX after `div rbx`?",
        setup: &[("rax", 17), ("rdx", 0), ("rbx", 5)],
        instruction: "div rbx",
        register: "rdx",
        answer: 2,
        explanation: "`div` divides RDX:RAX by its operand. The quotient, 3, goes to RAX and the \
                      remainder, 2, to RDX. Had RDX held anything but zero, it would have been \
                      part of the dividend.",
        flags_set: &[],
    },
    Question {
        id: "cqo",
        prompt: "RAX holds -5. What is in RDX after `cqo`?",
        setup: &[("rax", 0xffff_ffff_ffff_fffb), ("rdx", 0)],
        instruction: "cqo",
        register: "rdx",
        answer: 0xffff_ffff_ffff_ffff,
        explanation: "`cqo` copies the sign bit of RAX into every bit of RDX, extending RAX into \
                      the 128-bit RDX:RAX that `idiv` divides. A negative RAX gives all ones.",
        flags_set: &[],
    },
    Question {
        id: "test-odd",
        prompt: "RAX holds 6. What is in RAX after `test rax, 1`?",
        setup: &[("rax", 6)],
        instruction: "test rax, 1",
        register: "rax",
        answer: 6,
        explanation: "`test` computes an AND only to set the flags and writes nothing back, so \
                      RAX keeps 6. The AND of 6 and 1 is zero, so ZF is set: 6 is even.",
        flags_set: &[Flag::Zero],
    },
    Question {
        id: "or-set",
        prompt: "RAX holds 0x10. What is in RAX after `or rax, 0x5`?",
        setup: &[("rax", 0x10)],
        instruction: "or rax, 0x5",
        register: "rax",
        answer: 0x15,
        explanation: "`or` sets every bit that is set in either operand, so the bits of 0x5 are \
                      added to 0x10, giving 0x15.",
        flags_set: &[],
    },
    Question {
        id: "movsx",
        prompt: "RBX holds 0x80. What is in RAX after `movsx rax, bl`?",
        setup: &[("rax", 0), ("rbx", 0x80)],
        instruction: "movsx rax, bl",
        register: "rax",
        answer: 0xffff_ffff_ffff_ff80,
        explanation: "`movsx` treats the byte as signed. 0x80 has its top bit set, so it is -128, \
                      and sign extension fills the upper bits with ones. `movzx` would have given \
                      0x80.",
        flags_set: &[],
    },
    Question {
        id: "rol",
        prompt: "RAX holds 0x8000000000000001. What is in RAX after `rol rax, 1`?",
        setup: &[("rax", 0x8000_0000_0000_0001)],
        instruction: "rol rax, 1",
        register: "rax",
        answer: 3,
        explanation: "A rotate moves the bit that falls off the top back in at the bottom, so \
                      the high bit becomes bit 0 and the old bit 0 moves to bit 1: 0b11. The \
                      rotated bit is also copied into CF.",
        flags_set: &[Flag::Carry],
    },
    Question {
        id: "dec-loop",
        prompt: "RCX holds 1. What is in RCX after `dec rcx`?",
        setup: &[("rcx", 1)],
        instruction: "dec rcx",
        register: "rcx",
        answer: 0,
        explanation: "The counter reaches zero and ZF is set, so a following `jnz` falls \
                      through and the loop ends. That is why counting down needs no `cmp`.",
        flags_set: &[Flag::Zero],
    },
    Question {
        id: "xchg",
        prompt: "RAX holds 1 and RBX holds 2. What is in RAX after `xchg rax, rbx`?",
        setup: &[("rax", 1), ("rbx", 2)],
        instruction: "xchg rax, rbx",
        register: "rax",
        answer: 2,
        explanation: "`xchg` swaps its operands in one instruction, so RAX takes RBX's 2 and RBX \
                      takes 1. With a memory operand it is also atomic, which makes it a lock \
                      primitive.",
        flags_set: &[],
    },
];

/// The lesson with an identifier.
pub fn lesson(id: &str) -> Option<&'static Lesson> {
    LESSONS.iter().find(|lesson| lesson.id == id)
}

/// The question with an identifier.
pub fn question(id: &str) -> Option<&'static Question> {
    QUESTIONS.iter().find(|question| question.id == id)
}

/// Whether an answer has been given, and whether it was right.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing has been answered yet.
    #[default]
    Unanswered,
    /// The answer was right.
    Correct,
    /// The answer was wrong; the worked explanation is shown.
    Wrong {
        /// What was typed.
        given: String,
    },
}

/// One item of material: something to read, or something to answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    /// A lesson, by index into [`LESSONS`].
    Lesson(usize),
    /// A question, by index into [`QUESTIONS`].
    Question(usize),
}

/// Every item, lessons first and then questions.
///
/// One sequence rather than two modes: there is nothing to toggle, the arrow
/// keys move through all of it, and no key has to be stolen from panel
/// navigation to switch between them.
pub fn items() -> Vec<Item> {
    (0..LESSONS.len())
        .map(Item::Lesson)
        .chain((0..QUESTIONS.len()).map(Item::Question))
        .collect()
}

/// How many items there are in total.
pub fn item_count() -> usize {
    LESSONS.len() + QUESTIONS.len()
}

/// The reader's position in the material.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Progress {
    /// Which item is open.
    position: usize,
    /// The answer typed so far.
    pub typed: String,
    /// Whether the current question has been answered.
    pub verdict: Verdict,
    /// Questions answered correctly, by identifier.
    pub solved: Vec<&'static str>,
}

impl Progress {
    /// Starts at the beginning.
    pub fn new() -> Self {
        Self::default()
    }

    /// Which item is open.
    pub fn item(&self) -> Item {
        items()[self.position.min(item_count() - 1)]
    }

    /// The one-based position, for a progress indicator.
    pub fn position(&self) -> usize {
        self.position.min(item_count() - 1) + 1
    }

    /// The lesson currently open, when the item is one.
    pub fn current_lesson(&self) -> Option<&'static Lesson> {
        match self.item() {
            Item::Lesson(index) => LESSONS.get(index),
            Item::Question(_) => None,
        }
    }

    /// The question currently open, when the item is one.
    pub fn current_question(&self) -> Option<&'static Question> {
        match self.item() {
            Item::Question(index) => QUESTIONS.get(index),
            Item::Lesson(_) => None,
        }
    }

    /// Whether the open item expects an answer.
    pub fn is_question(&self) -> bool {
        matches!(self.item(), Item::Question(_))
    }

    /// Moves to the next item, wrapping around.
    pub fn next(&mut self) {
        self.position = (self.position + 1) % item_count();
        self.reset_answer();
    }

    /// Moves to the previous item, wrapping around.
    pub fn previous(&mut self) {
        self.position = (self.position + item_count() - 1) % item_count();
        self.reset_answer();
    }

    /// Jumps to the first question, for a reader who wants to test themselves.
    pub fn jump_to_questions(&mut self) {
        self.position = LESSONS.len();
        self.reset_answer();
    }

    /// Judges the typed answer.
    ///
    /// A wrong answer does not move on: the worked explanation appears and the
    /// reader can try again, which is the point of asking in the first place.
    pub fn submit(&mut self) {
        let Some(question) = self.current_question() else {
            return;
        };
        if question.is_correct(&self.typed) {
            self.verdict = Verdict::Correct;
            if !self.solved.contains(&question.id) {
                self.solved.push(question.id);
            }
        } else {
            self.verdict = Verdict::Wrong {
                given: self.typed.clone(),
            };
        }
    }

    /// Clears the typed answer and the verdict.
    pub fn reset_answer(&mut self) {
        self.typed.clear();
        self.verdict = Verdict::Unanswered;
    }

    /// How many questions have been answered correctly.
    pub fn solved_count(&self) -> usize {
        self.solved.len()
    }

    /// Whether every question has been answered correctly.
    pub fn is_complete(&self) -> bool {
        self.solved.len() == QUESTIONS.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_unique() {
        let mut ids: Vec<&str> = LESSONS.iter().map(|lesson| lesson.id).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate lesson identifier");

        let mut ids: Vec<&str> = QUESTIONS.iter().map(|question| question.id).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate question identifier");
    }

    #[test]
    fn every_lesson_has_content() {
        for lesson in LESSONS {
            assert!(!lesson.title.is_empty(), "{} has no title", lesson.id);
            assert!(!lesson.body.is_empty(), "{} has no body", lesson.id);
            for paragraph in lesson.body {
                assert!(paragraph.len() > 40, "{}: paragraph too thin", lesson.id);
            }
        }
    }

    #[test]
    fn the_spec_topics_are_all_covered() {
        let ids: Vec<&str> = LESSONS.iter().map(|lesson| lesson.id).collect();
        for topic in [
            "registers",
            "syscalls",
            "stack",
            "flags",
            "calling-convention",
        ] {
            assert!(ids.contains(&topic), "no lesson on {topic}");
        }
    }

    #[test]
    fn every_question_names_real_registers() {
        for question in QUESTIONS {
            assert!(
                crate::instruction::registers::is_register(question.register),
                "{}: {} is not a register",
                question.id,
                question.register
            );
            for (name, _) in question.setup {
                assert!(
                    crate::instruction::registers::is_register(name),
                    "{}: {name} is not a register",
                    question.id
                );
            }
        }
    }

    #[test]
    fn every_question_has_a_worked_explanation() {
        for question in QUESTIONS {
            assert!(!question.prompt.is_empty(), "{}", question.id);
            assert!(
                question.explanation.len() > 60,
                "{}: the explanation is too thin to teach anything",
                question.id
            );
        }
    }

    #[test]
    fn answers_are_accepted_in_any_reasonable_notation() {
        let question = question("add").expect("the add question");
        for typed in ["3", "0x3", "0b11", " 3 "] {
            assert!(question.is_correct(typed), "{typed:?} should be accepted");
        }
        assert!(!question.is_correct("4"));
        assert!(!question.is_correct(""));
        assert!(!question.is_correct("nonsense"));
    }

    #[test]
    fn a_negative_answer_is_read_as_its_bit_pattern() {
        // Someone reasoning about `sar` writes -4; someone reasoning about
        // bits writes the full hexadecimal. Both are the same answer.
        let question = question("sar-negative").expect("the sar question");
        assert!(question.is_correct("-4"));
        assert!(question.is_correct("0xfffffffffffffffc"));
    }

    #[test]
    fn malformed_answers_never_panic() {
        for text in ["", "   ", "0x", "0b", "-", "--5", "0xzz", &"9".repeat(400)] {
            let _ = parse_answer(text);
        }
    }

    #[test]
    fn the_material_is_one_sequence_of_lessons_then_questions() {
        // No mode to toggle means no key has to be stolen from panel
        // navigation to switch between them.
        let all = items();
        assert_eq!(all.len(), LESSONS.len() + QUESTIONS.len());
        assert!(matches!(all[0], Item::Lesson(0)));
        assert!(matches!(all[LESSONS.len()], Item::Question(0)));
    }

    #[test]
    fn progress_moves_and_wraps_in_both_directions() {
        let mut progress = Progress::new();
        assert_eq!(progress.position(), 1);
        assert!(progress.current_lesson().is_some());

        progress.previous();
        assert_eq!(progress.position(), item_count(), "wraps to the end");
        assert!(
            progress.current_question().is_some(),
            "the last item is a question"
        );

        progress.next();
        assert_eq!(progress.position(), 1, "wraps back to the start");
    }

    #[test]
    fn walking_the_whole_sequence_reaches_every_item() {
        let mut progress = Progress::new();
        let mut lessons = 0;
        let mut questions = 0;

        for _ in 0..item_count() {
            if progress.current_lesson().is_some() {
                lessons += 1;
            }
            if progress.current_question().is_some() {
                questions += 1;
            }
            progress.next();
        }
        assert_eq!(lessons, LESSONS.len());
        assert_eq!(questions, QUESTIONS.len());
    }

    #[test]
    fn jumping_to_the_questions_skips_the_lessons() {
        let mut progress = Progress::new();
        progress.jump_to_questions();
        assert!(progress.is_question());
        assert_eq!(
            progress.current_question().map(|q| q.id),
            Some(QUESTIONS[0].id)
        );
    }

    #[test]
    fn a_lesson_has_no_question_to_answer() {
        let progress = Progress::new();
        assert!(!progress.is_question());
        assert!(progress.current_question().is_none());
    }

    #[test]
    fn a_correct_answer_is_recorded_once() {
        let mut progress = Progress::new();
        progress.jump_to_questions();
        progress.typed = "3".to_owned();

        progress.submit();
        assert_eq!(progress.verdict, Verdict::Correct);
        assert_eq!(progress.solved_count(), 1);

        progress.submit();
        assert_eq!(progress.solved_count(), 1, "answering twice counts once");
    }

    #[test]
    fn a_wrong_answer_keeps_the_question_open() {
        // The explanation is the point; moving on would skip it.
        let mut progress = Progress::new();
        progress.jump_to_questions();
        let before = progress.current_question().map(|q| q.id);

        progress.typed = "99".to_owned();
        progress.submit();

        assert!(matches!(progress.verdict, Verdict::Wrong { .. }));
        assert_eq!(progress.current_question().map(|q| q.id), before);
        assert_eq!(progress.solved_count(), 0);
    }

    #[test]
    fn submitting_on_a_lesson_does_nothing() {
        let mut progress = Progress::new();
        progress.typed = "3".to_owned();
        progress.submit();
        assert_eq!(progress.verdict, Verdict::Unanswered);
    }

    #[test]
    fn moving_between_items_clears_the_previous_answer() {
        let mut progress = Progress::new();
        progress.jump_to_questions();
        progress.typed = "3".to_owned();
        progress.submit();

        progress.next();
        assert!(progress.typed.is_empty());
        assert_eq!(progress.verdict, Verdict::Unanswered);
    }

    #[test]
    fn completion_needs_every_question() {
        let mut progress = Progress::new();
        assert!(!progress.is_complete());
        for question in QUESTIONS {
            progress.solved.push(question.id);
        }
        assert!(progress.is_complete());
    }

    #[test]
    fn an_out_of_range_position_is_clamped_rather_than_panicking() {
        let mut progress = Progress::new();
        for _ in 0..item_count() * 3 {
            progress.next();
        }
        let _ = progress.item();
        let _ = progress.current_lesson();
        let _ = progress.current_question();
        assert!(progress.position() <= item_count());
    }

    /// Runs every question on the real processor and checks the stated answer.
    ///
    /// This is the test that makes the material trustworthy: the questions
    /// cover precisely the cases people get wrong, so an answer typed from
    /// memory would be worse than no question at all.
    #[tokio::test]
    async fn every_stated_answer_matches_what_the_processor_does() {
        use std::path::Path;
        for tool in ["nasm", "ld", "gdb"] {
            if !crate::process::is_available(Path::new(tool)) {
                eprintln!("skipping: {tool} not installed");
                return;
            }
        }

        for question in QUESTIONS {
            let outcome = question
                .to_scratchpad()
                .run("gdb")
                .await
                .unwrap_or_else(|error| panic!("{}: {error}", question.id));

            // The register either changed to the stated answer, or it was
            // already that value and so does not appear in the diff.
            let actual = outcome
                .changes
                .iter()
                .find(|change| change.register == question.register)
                .map(|change| change.after)
                .unwrap_or_else(|| {
                    question
                        .setup
                        .iter()
                        .find(|(name, _)| *name == question.register)
                        .map(|(_, value)| *value)
                        .unwrap_or(0)
                });

            assert_eq!(
                actual, question.answer,
                "{}: the material says {:#x} but the processor produced {actual:#x}",
                question.id, question.answer
            );

            for flag in question.flags_set {
                assert!(
                    outcome.flags_after.has(*flag),
                    "{}: the material claims {flag} is set, but it is not",
                    question.id
                );
            }
        }
    }
}
