use comfy_table::{Cell, Color, Row, Table};
use kalosm::language::*;
use kalosm::*;
use std::path::PathBuf;
use surrealdb::{engine::local::RocksDb, Surreal};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let exists = std::path::Path::new("./db").exists();

    // Create database connection
    let db = Surreal::new::<RocksDb>("./db/temp.db").await.unwrap();

    // Select a specific namespace / database
    db.use_ns("test").use_db("test").await.unwrap();

    let mut document_table = db
        .document_table_builder("documents")
        .at("./db/embeddings.db")
        .build()
        .await
        .unwrap();

    if !exists {
        std::fs::create_dir_all("documents").unwrap();
        let documents = DocumentFolder::try_from(PathBuf::from("./documents")).unwrap();

        // Create a new document database table
        let documents = documents.into_documents().await.unwrap();
        for document in documents {
            document_table.insert(document).await.unwrap();
        }
    }

    loop {
        let user_question = prompt_input("Query: ").unwrap();
        let user_question_embedding = document_table
            .embedding_model_mut()
            .embed(&user_question)
            .await
            .unwrap();

        let nearest_5 = document_table
            .select_nearest_embedding(user_question_embedding, 5)
            .await
            .unwrap();

        let mut table = Table::new();
        table.set_header(vec!["Score", "Value"]);

        for result in nearest_5 {
            let mut row = Row::new();
            let color = if result.distance < 0.25 {
                Color::Green
            } else if result.distance < 0.75 {
                Color::Yellow
            } else {
                Color::Red
            };
            row.add_cell(Cell::new(result.distance).fg(color))
                .add_cell(Cell::new(result.record.body()[0..50].to_string() + "..."));
            table.add_row(row);
        }

        println!("{}", table);
    }
}
