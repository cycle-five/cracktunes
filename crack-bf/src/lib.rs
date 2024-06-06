use std::{
    io::{BufRead, Write},
    sync::{atomic::AtomicBool, Arc},
};

use tokio::io::{
    AsyncBufRead, AsyncWrite, {AsyncReadExt, AsyncWriteExt},
};
pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// Representation of a brainfuck program.
#[derive(Clone, Debug)]
pub struct BrainfuckProgram {
    pub code: Vec<u8>,
    pub cells: [u8; 30000],
    pub ptr: usize,
    pub pc: usize,
    pub done: Arc<AtomicBool>,
}

/// Implementation of the representation and execution of a brainfuck program.
impl BrainfuckProgram {
    pub fn new(program: String) -> Self {
        let program = program
            .chars()
            .filter(|c| matches!(c, '+' | '-' | '<' | '>' | '[' | ']' | '.' | ','))
            .collect::<String>();
        let code = program.as_bytes();

        Self {
            code: code.to_vec(),
            cells: [0u8; 30000],
            ptr: 0,
            pc: 0,
            done: Arc::new(AtomicBool::default()),
        }
    }

    /// Match a bracket b'[' to the matching b']' on the same level of
    /// precedence. Returns the new PC pointer.
    /// TODO: Add some error handling.
    fn match_bracket_forward(cells: &[u8], ptr: usize, code: &[u8], pc: usize) -> usize {
        let mut pc = pc;
        if cells[ptr] == 0 {
            let mut depth = 1;
            while depth > 0 {
                pc += 1;
                if code[pc] == b'[' {
                    depth += 1;
                } else if code[pc] == b']' {
                    depth -= 1;
                }
            }
        }
        pc
    }

    /// Match a bracket b']' to the matching b'[' on the same level of
    /// precedence. Returns the new PC pointer.
    /// TODO: Add some error handling.
    fn match_bracket_backward(cells: &[u8], ptr: usize, code: &[u8], pc: usize) -> usize {
        let mut pc = pc;
        if cells[ptr] != 0 {
            let mut depth = 1;
            while depth > 0 {
                pc -= 1;
                if code[pc] == b'[' {
                    depth -= 1;
                } else if code[pc] == b']' {
                    depth += 1;
                }
            }
        }
        pc
    }

    /// Write the current cell to the writer.
    #[allow(dead_code)]
    fn write_cell(&self, writer: &mut impl Write) -> Result<(), Error> {
        let val = self.cells[self.ptr];
        match writer.write_all(&[val]) {
            Ok(_) => Ok(()),
            Err(e) => Err(Box::new(e)),
        }
    }

    // async fn write_cell_async(&self, writer: &mut impl AsyncWrite) -> Result<(), Error> {
    //     let val = self.cells[self.ptr];
    //     match w {
    //         Ok(_) => Ok(()),
    //         Err(e) => Err(Box::new(e)),
    //     }
    // }

    /// Run the brainfuck program.
    pub async fn run_async<R, W>(&mut self, mut reader: R, mut writer: W) -> Result<(), Error>
    where
        R: AsyncBufRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        let mut cells = self.cells;
        let code = &self.code.clone();
        let mut pc = 0;
        let mut ptr = 0;
        while pc < code.len() {
            match code[pc] {
                b'+' => cells[ptr] = cells[ptr].wrapping_add(1),
                b'-' => cells[ptr] = cells[ptr].wrapping_sub(1),
                b'<' => ptr = ptr.wrapping_sub(1),
                b'>' => ptr = ptr.wrapping_add(1),
                b'[' => pc = Self::match_bracket_forward(&cells, ptr, code, pc),
                b']' => pc = Self::match_bracket_backward(&cells, ptr, code, pc),
                b'.' => {
                    let val = cells[ptr];
                    match writer.write_all(&[val]).await {
                        Ok(_) => {},
                        Err(e) => {
                            self.done.store(true, std::sync::atomic::Ordering::Relaxed);
                            return Err(Box::new(e));
                            // self
                        },
                    }
                },
                b',' => {
                    let mut input = [0u8; 1];
                    match reader.read_exact(&mut input).await {
                        Ok(_) => {
                            cells[ptr] = input[0];
                        },
                        Err(_) => {
                            cells[ptr] = 0;
                        },
                    }
                },
                _ => {},
            }
            pc += 1;
        }
        self.done.store(true, std::sync::atomic::Ordering::Relaxed);
        // self
        Ok(())
    }

    /// Run the brainfuck program.
    pub fn run<R, W>(&mut self, mut reader: R, mut writer: W) -> Result<(), Error>
    where
        R: BufRead,
        W: Write,
    {
        let mut cells = self.cells;
        let code = &self.code.clone();
        let mut pc = 0;
        let mut ptr = 0;
        while pc < code.len() {
            match code[pc] {
                b'+' => cells[ptr] = cells[ptr].wrapping_add(1),
                b'-' => cells[ptr] = cells[ptr].wrapping_sub(1),
                b'<' => ptr = ptr.wrapping_sub(1),
                b'>' => ptr = ptr.wrapping_add(1),
                b'[' => pc = Self::match_bracket_forward(&cells, ptr, code, pc),
                b']' => pc = Self::match_bracket_backward(&cells, ptr, code, pc),
                b'.' => {
                    let val = cells[ptr];
                    match writer.write_all(&[val]) {
                        Ok(_) => {},
                        Err(e) => {
                            self.done.store(true, std::sync::atomic::Ordering::Relaxed);
                            return Err(Box::new(e));
                            // self
                        },
                    }
                },
                b',' => {
                    let mut input = [0u8; 1];
                    match reader.read_exact(&mut input) {
                        Ok(_) => {
                            cells[ptr] = input[0];
                        },
                        Err(_) => {
                            cells[ptr] = 0;
                        },
                    }
                },
                _ => {},
            }
            pc += 1;
        }
        self.done.store(true, std::sync::atomic::Ordering::Relaxed);
        // self
        Ok(())
    }
}

// impl Future for BrainfuckProgram {
//     type Output = Result<(), Error>;

//     fn poll(
//         self: std::pin::Pin<&mut Self>,
//         _cx: &mut std::task::Context<'_>,
//     ) -> std::task::Poll<Self::Output> {
//         if self.done.load(std::sync::atomic::Ordering::Relaxed) {
//             Poll::Ready(Ok(()))
//         } else {
//             Poll::Pending
//         }
//     }
// }

#[cfg(test)]
mod tests {
    use std::io::BufReader;

    use super::*;

    use std::io::Cursor;

    #[test]
    fn test_hello_world_cursor() {
        let program = String::from("++++++++++[>+++++++>++++++++++>+++>+<<<<-]>++.>+.+++++++..+++.>++.<<+++++++++++++++.>.+++.------.--------.>+.>.");
        let mut bf = BrainfuckProgram::new(program);

        let input = Cursor::new(vec![]);
        let mut output = Cursor::new(vec![]);

        bf.run(input, &mut output).unwrap();

        let result = String::from_utf8(output.into_inner()).unwrap();
        assert_eq!(result, "Hello World!\n");
    }

    #[test]
    fn test_input_output() {
        let program = String::from(",[.,]");
        let mut bf = BrainfuckProgram::new(program);

        let input_data = b"Brainfuck\n".to_vec();
        let input = Cursor::new(input_data);
        let mut output = Cursor::new(vec![]);

        bf.run(input, &mut output).unwrap();

        let result = String::from_utf8(output.into_inner()).unwrap();
        println!("{}", result.clone());
        assert_eq!(result, "Brainfuck\n");
    }

    #[tokio::test]
    async fn test_input_output_async() {
        let program = String::from(",[.,]");
        let mut bf = BrainfuckProgram::new(program);

        let input_data = b"Brainfuck\n".to_vec();
        let input = Cursor::new(input_data);
        let mut output = Cursor::new(vec![]);

        let _ = bf.run_async(input, &mut output).await;

        let result = String::from_utf8(output.into_inner()).unwrap();
        println!("{}", result.clone());
        assert_eq!(result, "Brainfuck\n");
    }

    #[test]
    fn test_hello_world() {
        let program = r#"
            ++++++++++[>+++++++>++++++++++>+++>+<<<<-]>++.>+.+++++++..+++.>++.<<+++++++++++++++.>.+++.------.--------.>+.>.
        "#;
        let stdio = std::io::stdin();
        let stdin = stdio.lock();
        let stdout = std::io::stdout();
        let mut bf = BrainfuckProgram::new(program.to_string());
        if let Err(_) = bf.run(stdin, stdout) {
            assert!(false)
        }
    }

    #[tokio::test]
    async fn test_calculator() {
        let program = r#"
            +>+>+>+>>>,.>++++[<---------->-]<-------[-<[>>+<<-]>>[<<++++++++++>>-]<[<+>-],.>++++[<---------->-]<--[>+<-]>[<<<<<<<->>>>>>>-[<<<<<<->>>>>>--[<<<<<->>>>>--[<<<<<<<+>+>+>>>>>[<+>-]]]]]<]>,.>++++[<---------->-]<-------[-<[>>+<<-]>>[<<++++++++++>>-]<[<+>-],.>++++[<---------->-]<-------[>+>+<<-]>>[<<+>>-]<-[-[-[-[-[-[-[-[-[-[<[-]>[-]]]]]]]]]]]<]<<<<<<<[->->->->>[>>+<<-]>[>[<<+>>>+<-]>[<+>-]<<-]>[-]<<<<<<<]>[->->->>>[<+>-]<<<<<]>[->->+>>[>+<-]>>+<[>-<[<+>-]]>[-<<<<->[>+<-]>>>]<<<[->-[>+<-]>>+<[>-<[<+>-]]>[-<<<<->[>+<-]>>>]<<<]>[<+>-]<<<<]>[->>>>>+[-<<<[>>>+>+<<<<-]>>>[<<<+>>>-]<<[>>+>>+<<<<-]>>[<<+>>-]>[->->>+<<[>+<-]>[>-<[<+>-]]>[-<<<<+<+<<[-]>>>>[<<<<+>>>>-]>>>]<<<]>[-]<<]<<[-]<[>+<-]>>[<<+>>-]<<<<]>>>[>>+[<<[>>>+>+<<<<-]>>>>[<<<<+>>>>-]+<[-[-[-[-[-[-[-[-[-[>-<<<<---------->+>>[-]]]]]]]]]]]>[->[>]>++++[<++++++++++>-]<++++++++[<]<<<<[>>>>>[>]<+[<]<<<<-]>>-<[>+<[<+>-]]>>>]<<]>>>[>]>++++[<++++++++++>-]<++++++>>++++[<++++++++++>-]<++++++>>++++[<++++++++++>-]<++++++[<]<<<<]>+[<<[>>>+>+<<<<-]>>>>[<<<<+>>>>-]+<[-[-[-[-[-[-[-[-[-[>-<<<<---------->+>>[-]]]]]]]]]]]>[->>[>]>++++[<++++++++++>-]<++++++++[<]<<<<<[>>>>>>[>]<+[<]<<<<<-]>>-<[>+<[<+>-]]>>>]<<]<<<[->>>>>>>[>]>++++[<++++++++++>-]<+++++[<]<<<<<<]>>>>>>>[>]<[.<]
        "#;
        let input = BufReader::new(&b"2+100\n"[..]);
        let stdout = std::io::stdout();
        let mut bf = BrainfuckProgram::new(program.to_string());
        let _ = bf.run(input, stdout);
    }

    #[tokio::test]
    async fn test_async() {
        let program = r#"
            >+++++++++++[-<+++++++++++++++>] # initialize 165 at first cell
            >++++++++++<<[->+>-[>+>>]>[+[-<+>]>+>>]<<<<<<]>>[-]>>>++++++++++<[->-[>+>>]>[+[-
            <+>]>+>>]<<<<<]>[-]>>[>++++++[-<++++++++>]<.<<+>+>[-]]<[<[->-<]++++++[->++++++++
            <]>.[-]]<<++++++[-<++++++++>]<.[-]<<[-<+>]
        "#;
        let input = Cursor::new(&b"100\n"[..]);
        let output = Cursor::new(vec![]);

        let mut bf = BrainfuckProgram::new(program.to_string());
        if let Err(_) = bf.run_async(input, output).await {
            assert!(false)
        }
    }
}
