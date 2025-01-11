use std::io::{BufRead, Write};
use tokio::io::{
    AsyncBufRead, AsyncWrite, {AsyncBufReadExt, AsyncWriteExt},
};
pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// Representation of a brainfuck program.
#[derive(Clone, Debug)]
pub struct BrainfuckProgram {
    pub code: Vec<u8>,
    pub cells: [u8; 30000],
    pub ptr: usize,
    pub pc: usize,
}

/// Implementation of the representation and execution of a brainfuck program.
#[allow(clippy::large_stack_arrays)]
impl BrainfuckProgram {
    #[must_use]
    pub fn new(program: &str) -> Self {
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
        }
    }

    /// Match a bracket 0b`[` to the matching 0b`]` on the same level of
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
            Ok(()) => Ok(()),
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
    /// # Errors
    /// * `Error` - Can error if there is an issue reading from the reader or writing to the writer.
    #[allow(clippy::match_on_vec_items)]
    pub async fn run_async<R, W>(&mut self, mut reader: R, mut writer: W) -> Result<usize, Error>
    where
        R: AsyncBufRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        let mut bytes_wrote = 0;
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
                    match writer.write(&[val]).await {
                        Ok(_) => {
                            bytes_wrote += 1;
                        },
                        Err(e) => {
                            writer.flush().await?;
                            return Err(Box::new(e));
                        },
                    }
                },
                b',' => match reader.fill_buf().await {
                    Ok(buf) => {
                        if buf.is_empty() {
                            cells[ptr] = 0;
                        } else {
                            cells[ptr] = buf[0];
                            reader.consume(1);
                        }
                    },
                    Err(_) => {
                        cells[ptr] = 0;
                    },
                },
                _ => {},
            }
            pc += 1;
        }
        Ok(bytes_wrote)
    }

    /// Run the brainfuck program.
    /// # Arguments
    /// * `reader` - A reader to read input (program source) from.
    /// * `writer` - A writer to write (program) output to.
    /// # Returns
    /// * `Result<(), Error>` - A result indicating success or failure.
    /// # Errors
    /// * `Error` - Can error if there is an issue reading from the reader or writing to the writer.
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
            #[allow(clippy::match_on_vec_items)]
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
                        Ok(()) => {},
                        Err(e) => {
                            // self.done.store(true, Ordering::Relaxed);
                            return Err(Box::new(e));
                            // self
                        },
                    }
                },
                b',' => {
                    let mut input = [0u8; 1];
                    match reader.read_exact(&mut input) {
                        Ok(()) => {
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

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::BufReader;
    use std::io::Cursor;

    #[test]
    fn test_hello_world_cursor() {
        let program = String::from("++++++++++[>+++++++>++++++++++>+++>+<<<<-]>++.>+.+++++++..+++.>++.<<+++++++++++++++.>.+++.------.--------.>+.>.");
        let mut bf = BrainfuckProgram::new(&program);

        let input = Cursor::new(vec![]);
        let mut output = Cursor::new(vec![]);

        bf.run(input, &mut output).unwrap();

        let result = String::from_utf8(output.into_inner()).unwrap();
        assert_eq!(result, "Hello World!\n");
    }

    #[test]
    fn test_input_output() {
        let program = ",[.,]";
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
        let program = ",[.,]";
        let mut bf = BrainfuckProgram::new(program);

        let input_data = b"Brainfuck\n".to_vec();
        let input = Cursor::new(input_data);
        let mut output = Cursor::new(vec![]);

        let res = Box::pin(bf.run_async(input, &mut output)).await;
        match res {
            Ok(n) => println!("Wooooo! {n}"),
            Err(_) => {
                println!("Boooo!");
            },
        };

        let result = String::from_utf8(output.into_inner()).unwrap();
        println!("{}", result.clone());
        assert_eq!(result, "Brainfuck\n");
    }

    #[test]
    fn test_hello_world() {
        let program = r"
            ++++++++++[>+++++++>++++++++++>+++>+<<<<-]>++.>+.+++++++..+++.>++.<<+++++++++++++++.>.+++.------.--------.>+.>.
        ";
        let stdin_init = std::io::stdin();
        let stdin = stdin_init.lock();
        let stdout = std::io::stdout();
        let mut bf = BrainfuckProgram::new(program);
        assert!(bf.run(stdin, stdout).is_ok(),);
    }

    #[tokio::test]
    async fn test_calculator() {
        let program = r"
            +>+>+>+>>>,.>++++[<---------->-]<-------[-<[>>+<<-]>>[<<++++++++++>>-]<[<+>-],.>++++[<---------->-]<--[>+<-]>[<<<<<<<->>>>>>>-[<<<<<<->>>>>>--[<<<<<->>>>>--[<<<<<<<+>+>+>>>>>[<+>-]]]]]<]>,.>++++[<---------->-]<-------[-<[>>+<<-]>>[<<++++++++++>>-]<[<+>-],.>++++[<---------->-]<-------[>+>+<<-]>>[<<+>>-]<-[-[-[-[-[-[-[-[-[-[<[-]>[-]]]]]]]]]]]<]<<<<<<<[->->->->>[>>+<<-]>[>[<<+>>>+<-]>[<+>-]<<-]>[-]<<<<<<<]>[->->->>>[<+>-]<<<<<]>[->->+>>[>+<-]>>+<[>-<[<+>-]]>[-<<<<->[>+<-]>>>]<<<[->-[>+<-]>>+<[>-<[<+>-]]>[-<<<<->[>+<-]>>>]<<<]>[<+>-]<<<<]>[->>>>>+[-<<<[>>>+>+<<<<-]>>>[<<<+>>>-]<<[>>+>>+<<<<-]>>[<<+>>-]>[->->>+<<[>+<-]>[>-<[<+>-]]>[-<<<<+<+<<[-]>>>>[<<<<+>>>>-]>>>]<<<]>[-]<<]<<[-]<[>+<-]>>[<<+>>-]<<<<]>>>[>>+[<<[>>>+>+<<<<-]>>>>[<<<<+>>>>-]+<[-[-[-[-[-[-[-[-[-[>-<<<<---------->+>>[-]]]]]]]]]]]>[->[>]>++++[<++++++++++>-]<++++++++[<]<<<<[>>>>>[>]<+[<]<<<<-]>>-<[>+<[<+>-]]>>>]<<]>>>[>]>++++[<++++++++++>-]<++++++>>++++[<++++++++++>-]<++++++>>++++[<++++++++++>-]<++++++[<]<<<<]>+[<<[>>>+>+<<<<-]>>>>[<<<<+>>>>-]+<[-[-[-[-[-[-[-[-[-[>-<<<<---------->+>>[-]]]]]]]]]]]>[->>[>]>++++[<++++++++++>-]<++++++++[<]<<<<<[>>>>>>[>]<+[<]<<<<<-]>>-<[>+<[<+>-]]>>>]<<]<<<[->>>>>>>[>]>++++[<++++++++++>-]<+++++[<]<<<<<<]>>>>>>>[>]<[.<]
        ";
        let input = BufReader::new(&b"2+100\n"[..]);
        let stdout = std::io::stdout();
        let mut bf = BrainfuckProgram::new(program);
        let _ = bf.run(input, stdout);
    }

    #[tokio::test]
    async fn test_async() {
        let program = r"
            >+++++++++++[-<+++++++++++++++>] # initialize 165 at first cell
            >++++++++++<<[->+>-[>+>>]>[+[-<+>]>+>>]<<<<<<]>>[-]>>>++++++++++<[->-[>+>>]>[+[-
            <+>]>+>>]<<<<<]>[-]>>[>++++++[-<++++++++>]<.<<+>+>[-]]<[<[->-<]++++++[->++++++++
            <]>.[-]]<<++++++[-<++++++++>]<.[-]<<[-<+>]
        ";
        let input = Cursor::new(&b"100\n"[..]);
        let output = Cursor::new(vec![]);

        let mut bf = BrainfuckProgram::new(program);
        assert!(Box::pin(bf.run_async(input, output)).await.is_ok(), "Error running program");
    }
}
