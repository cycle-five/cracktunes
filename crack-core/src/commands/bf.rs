use crate::messaging::message::CrackedMessage;
use crate::{utils::send_reply, Context, CrackedError, Error};
use crack_bf::BrainfuckProgram;
use poise::ReplyHandle;
use std::io::Cursor;
use std::time::Duration;
use tokio::time::timeout;

/// Brainfk interpreter.
#[poise::command(slash_command, prefix_command)]
pub async fn bf(
    ctx: Context<'_>,
    #[description = "Brainfk program to run."] program: String,
    #[rest]
    #[description = "Optional input to feed to the program on stdin."]
    input: Option<String>,
) -> Result<(), Error> {
    bf_internal(ctx, program, input.unwrap_or_default())
        .await
        .map(|_| ())
        .map_err(Into::into)
}

// /// Select one of several stored brainfuck programs to load and run, then
// /// print the program source code.
// #[poise::command(slash_command, prefix_command)]
// pub async fn bf_select(ctx: Context<'_>) -> Result<(), Error> {
//     let msg = send_brainfk_options(&ctx).await;
//     // let selection =
// }

// async fn send_brainfk_options(ctx: Context<'_>) -> Result<Message, Error> {}

/// Run a brainfk program. Program and input string maybe empty,
/// no handling is done for invalid programs.
pub async fn bf_internal(
    ctx: Context<'_>,
    program: String,
    input: String,
) -> Result<ReplyHandle, CrackedError> {
    tracing::info!("program: {program}, input: {input}");
    let mut bf = BrainfuckProgram::new(program);

    let arr_u8 = input.as_bytes();
    //let user_input = arr_u8;
    let user_input = Cursor::new(arr_u8);
    let mut output = Cursor::new(Vec::<u8>::with_capacity(32));

    // let handle = HANDLE.lock().unwrap().clone().unwrap();
    //tokio::task::block_in_place(move || handle.block_on(async { bf.run(user_input, &mut output)).await }

    let n = timeout(
        Duration::from_secs(30),
        bf.run_async(user_input, &mut output),
    )
    .into_inner()
    .await?;

    let string_out = cursor_to_string(output, n)?;
    tracing::info!("string_out\n{string_out}");
    let final_out = format!("```{string_out}```");
    // let ctx_clone = ctx;
    send_reply(&ctx, CrackedMessage::Other(final_out), false).await
}

// async fn cursor_to_string(mut cur: Cursor<Vec<u8>>, n: usize) -> Result<String, Error> {
//     //let mut output = Vec::with_capacity(n);
//     let output = String::new();
//     let x = cur.into_inner().fill_buf().await?;
//     tracing::info!("length: {}", x.len());
//     assert_eq!(n, x.len());
//     Ok(output)
// }

fn cursor_to_string(cur: Cursor<Vec<u8>>, n: usize) -> Result<String, Error> {
    //let mut output = Vec::with_capacity(n);
    let x = cur.into_inner();
    tracing::info!("length: {}", x.len());
    assert_eq!(n, x.len());
    Ok(String::from_utf8_lossy(&x).to_string())
}

#[allow(dead_code)]
fn ascii_art_number() -> String {
    let program = r#"
    >>>>+>+++>+++>>>>>+++[
        >,+>++++[>++++<-]>[<<[-[->]]>[<]>-]<<[
            >+>+>>+>+[<<<<]<+>>[+<]<[>]>+[[>>>]>>+[<<<<]>-]+<+>>>-[
            <<+[>]>>+<<<+<+<--------[
                <<-<<+[>]>+<<-<<-[
                <<<+<-[>>]<-<-<<<-<----[
                    <<<->>>>+<-[
                    <<<+[>]>+<<+<-<-[
                        <<+<-<+[>>]<+<<<<+<-[
                        <<-[>]>>-<<<-<-<-[
                            <<<+<-[>>]<+<<<+<+<-[
                            <<<<+[>]<-<<-[
                                <<+[>]>>-<<<<-<-[
                                >>>>>+<-<<<+<-[
                                    >>+<<-[
                                    <<-<-[>]>+<<-<-<-[
                                        <<+<+[>]<+<+<-[
                                        >>-<-<-[
                                            <<-[>]<+<++++[<-------->-]++<[
                                            <<+[>]>>-<-<<<<-[
                                                <<-<<->>>>-[
                                                <<<<+[>]>+<<<<-[
                                                    <<+<<-[>>]<+<<<<<-[
                                                    >>>>-<<<-<-
        ]]]]]]]]]]]]]]]]]]]]]]>[>[[[<<<<]>+>>[>>>>>]<-]<]>>>+>>>>>>>+>]<
    ]<[-]<<<<<<<++<+++<+++[
        [>]>>>>>>++++++++[<<++++>++++++>-]<-<<[-[<+>>.<-]]<<<<[
            -[-[>+<-]>]>>>>>[.[>]]<<[<+>-]>>>[<<++[<+>--]>>-]
            <<[->+<[<++>-]]<<<[<+>-]<<<<
        ]>>+>>>--[<+>---]<.>>[[-]<<]<
    ]
    "#;
    program.to_string()
}
