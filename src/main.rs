use std::collections::HashMap;

use crate::db::{genre::insert_genre, manga::get_manga};
use futures::join;
use mangaverse_entity::models::{genre::Genre, source::SourceTable};
use sqlx::mysql::MySqlPoolOptions;
use thiserror::Error;
use tuple_conv::RepeatedTuple;

pub mod db;
pub mod mangadino;
pub mod manganelo;
pub mod readm;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Text Parse Error")]
    TextParseError,

    #[error(transparent)]
    SQLError(#[from] sqlx::Error),

    #[error(transparent)]
    NetworkError(#[from] isahc::Error),

    #[error(transparent)]
    IOError(#[from] std::io::Error),

    #[error(transparent)]
    OtherError(#[from] Box<dyn std::error::Error>),

    #[error("You shouldn't be seeing this")]
    NoError,
}

#[derive(Default, Debug)]
pub struct Context {
    sources: HashMap<String, SourceTable>,
    genres: HashMap<String, Genre>,
}

async fn setup_db() -> Result<sqlx::Pool<sqlx::MySql>> {
    let configs = dotenvy::dotenv_iter().expect("No env file found");

    let db_url = configs
        .filter_map(std::result::Result::ok)
        .find(|f| f.0 == "DATABASE_URL")
        .expect("DATABASE_URL must be set")
        .1;

    let pool = MySqlPoolOptions::new()
        .max_connections(5)
        .connect(db_url.as_str())
        .await?;
    Ok(pool)
}

#[async_std::main]
async fn main() -> Result<()> {
    println!("Hello, world!");

    let pool = setup_db().await?;

    let mut c = Context::default();

    let f1 = manganelo::entity::get_manganelo_genres();
    let f2 = readm::entity::get_readm_genres();
    let f3 = mangadino::entity::get_mangadino_genres();
    let r = join!(f1, f2, f3)
        .to_vec()
        .into_iter()
        .filter_map(Result::ok)
        .flatten()
        .collect();

    insert_genre(&r, &pool, &mut c.genres).await?;

    let g1 = manganelo::entity::get_manganelo_source(&pool);
    let g2 = readm::entity::get_readm_source(&pool);
    let g3 = mangadino::entity::get_manganelo_source(&pool);

    c.sources = join!(g1, g2, g3)
        .to_vec()
        .into_iter()
        .filter_map(Result::ok)
        .map(|f| (f.name.clone(), f))
        .collect();

    println!("{:#?}", c);

    let r = manganelo::entity::get_manga(
        String::from("https://manganato.com/manga-dh981316"),
        &c.sources["manganelo"],
        &c.genres,
    )
    .await?;
    println!("{:#?}", r);

    let r2 = readm::entity::get_manga(
        String::from("https://readm.org/manga/19986"),
        &c.sources["readm"],
        &c.genres,
    )
    .await?;

    println!("{:#?}", r2);

    // let r2 = readm::entity::get_manga(
    //     String::from("https://readm.org/manga/magic-emperor"),
    //     &c.sources["readm"],
    //     &c.genres,
    // )
    // .await?;

    // println!("{:#?}", r2);

    println!(
        "{:#?}",
        get_manga("https://manganato.com/manga-dh981316", &pool, &c).await?
    );

    Ok(())
}
