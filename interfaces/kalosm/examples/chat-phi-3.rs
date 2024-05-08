use kalosm::language::*;

#[tokio::main]
async fn main() {
    let model = Llama::builder()
        .with_source(LlamaSource::phi_3_mini_4k_instruct())
        .build()
        .await
        .unwrap();
    let mut chat = Chat::builder(model)
        .with_system_prompt("You will act like a pirate")
        .build();

    loop {
        chat.add_message(prompt_input("\n> ").unwrap())
            .await
            .unwrap()
            .to_std_out()
            .await
            .unwrap();
    }
}
