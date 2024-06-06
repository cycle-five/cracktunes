use crate::messaging::message::CrackedMessage;
use crate::{utils::send_response_poise_text, Context, CrackedError, Error};
use crack_bf::BrainfuckProgram;
use serenity::all::Message;
use std::io::{Cursor, Read};
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

/// Run a brainfk program. Program and input string maybe empty, no handling is done for invalid
/// programs.
pub async fn bf_internal(
    ctx: Context<'_>,
    program: String,
    input: String,
) -> Result<Message, CrackedError> {
    tracing::info!("program: {program}, input: {input}");
    let mut bf = BrainfuckProgram::new(program);

    let arr_u8 = input.as_bytes();
    let user_input = Cursor::new(arr_u8);
    let mut output = Cursor::new(vec![]);

    // let handle = HANDLE.lock().unwrap().clone().unwrap();
    //tokio::task::block_in_place(move || handle.block_on(async { bf.run(user_input, &mut output)).await }

    let _ = timeout(Duration::from_secs(30), async {
        bf.run_async(user_input, &mut output).await
    })
    .await??;

    let string_out = cursor_to_string(output);
    tracing::info!("string_out: {string_out}");
    let final_out = format!("output: {string_out}");
    send_response_poise_text(ctx, CrackedMessage::Other(final_out)).await
}

fn cursor_to_string(mut cur: Cursor<Vec<u8>>) -> String {
    let mut output = String::new();
    let _ = cur.read_to_string(&mut output);
    output
}
