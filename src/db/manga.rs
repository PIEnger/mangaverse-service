use crate::{Context, Result};
use itertools::Itertools;
use lazy_static::lazy_static;
use mangaverse_entity::models::chapter::ChapterTable;
use mangaverse_entity::models::manga::MangaTable;
use mangaverse_entity::models::page::PageTable;
use mangaverse_entity::models::source::SourceTable;
use sqlx::mysql::MySqlRow;
use sqlx::types::chrono::NaiveDateTime;
use sqlx::{FromRow, MySql, Pool, Row};

lazy_static! {
    static ref JUNK_SOURCE: SourceTable = SourceTable {
        id: String::default(),
        name: String::default(),
        priority: 2,
    };
}

pub struct RowWrapper<T> {
    pub data: T,
}

pub struct MangaTableWrapper<'a> {
    pub contents: MangaTable<'a>,
    pub source_id: String,
}

impl FromRow<'_, MySqlRow> for MangaTableWrapper<'_> {
    fn from_row(row: &'_ MySqlRow) -> std::result::Result<Self, sqlx::Error> {
        Ok(MangaTableWrapper {
            contents: MangaTable {
                id: row.try_get("manga_id")?,
                linked_id: row.try_get("linked_id")?,
                is_listed: row.try_get("is_listed")?,
                name: row.try_get("name")?,
                cover_url: row.try_get("cover_url")?,
                url: row.try_get("url")?,
                last_updated: row.try_get("last_updated")?,
                status: row.try_get("status")?,
                is_main: row.try_get("is_main")?,
                description: row.try_get("description")?,
                last_watch_time: row.try_get("last_watch_time")?,
                public_id: row.try_get("public_id")?,
                is_old: row.try_get("is_old")?,
                source: &JUNK_SOURCE,
                chapters: Vec::default(),
                authors: Vec::default(),
                artists: Vec::default(),
                genres: Vec::default(),
                titles: Vec::default(),
            },
            source_id: row.try_get("source_id")?,
        })
    }
}

pub async fn update_manga(
    url: &str,
    mng: &mut MangaTable<'_>,
    pool: &Pool<MySql>,
    c: &Context,
) -> Result<()> {
    let stored = get_manga(url, pool, c).await?;

    let t = stored.name == mng.name
        && stored.cover_url == mng.cover_url
        && stored.last_updated == mng.last_updated
        && stored.status == mng.status
        && stored.description == mng.description;

    if !t {
        // update sql
        sqlx::query!("UPDATE manga SET name = ?, cover_url = ?, last_updated = ?, status = ?, description = ? where manga_id = ?", mng.name, mng.cover_url, mng.last_updated, mng.status, mng.description, stored.id).execute(pool).await?;
    }

    //handle collection updates probably by a generic function

    Ok(())
}

pub async fn get_manga<'a>(
    url: &'a str,
    pool: &'a Pool<MySql>,
    c: &'a Context,
) -> Result<MangaTable<'a>> {
    let mut r: MangaTableWrapper<'a> = sqlx::query_as("SELECT * from manga where url = ?")
        .bind(url)
        .fetch_one(pool)
        .await?;

    pub type RowWrapperString = RowWrapper<String>;

    r.contents.titles = sqlx::query_as!(
        RowWrapperString,
        "SELECT title as data from title where linked_id = ?",
        r.contents.linked_id
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|f| f.data)
    .collect();
    r.contents.authors = sqlx::query_as!(
        RowWrapperString,
        "SELECT author.name as data from author, manga_author where manga_author.author_id = author.author_id and manga_author.manga_id = ?",
        r.contents.id
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|f| f.data)
    .collect();
    r.contents.artists = sqlx::query_as!(
        RowWrapperString,
        "SELECT author.name as data from author, manga_artist where manga_artist.author_id = author.author_id and manga_artist.manga_id = ?",
        r.contents.id
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|f| f.data)
    .collect();
    r.contents.genres = sqlx::query_as!(
        RowWrapperString,
        "SELECT genre.name as data from genre, manga_genre where manga_genre.genre_id = genre.genre_id and manga_genre.manga_id = ?",
        r.contents.id
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .filter_map(|f| c.genres.get(f.data.as_str()))
    .collect();

    //TODO eliminate this call using the multi key hashmap which is still under development
    r.contents.source = c
        .sources
        .get(
            sqlx::query_as!(
                RowWrapperString,
                "SELECT name as data from source where source_id = ?",
                r.source_id
            )
            .fetch_one(pool)
            .await?
            .data
            .as_str(),
        )
        .unwrap();

    r.contents.chapters = get_chapters(r.contents.id.as_str(), pool).await?;

    Ok(r.contents)
}

pub async fn get_chapters(id: &str, pool: &Pool<MySql>) -> Result<Vec<ChapterTable>> {
    //do a hack
    //use group concat to eliminate multiple sql calls and speed shit up
    //use space as separator

    struct ChapterAndPages {
        pub chapter_id: String,
        pub chapter_name: String,
        pub chapter_number: String,
        pub updated_at: Option<NaiveDateTime>,
        pub manga_id: String,
        pub last_watch_time: i64,
        pub sequence_number: i32,

        pub all_pages: Option<String>,
    }

    let y = sqlx::query_as!(ChapterAndPages, "SELECT chapter.*, group_concat(chapter_page.chapter_page_id, ' ' ,chapter_page.url, ' ', chapter_page.page_number, ' ', chapter_page.chapter_id SEPARATOR ' ') as all_pages from chapter, chapter_page where chapter_page.chapter_id = chapter.chapter_id and chapter.manga_id = ? group by chapter_id", id).fetch_all(pool).await?;

    Ok(y.into_iter()
        .map(|f| ChapterTable {
            chapter_id: f.chapter_id,
            chapter_name: f.chapter_name,
            chapter_number: f.chapter_number,
            last_watch_time: f.last_watch_time,
            manga_id: f.manga_id,
            sequence_number: f.sequence_number,
            updated_at: f.updated_at,

            pages: f
                .all_pages
                .unwrap()
                .split_whitespace()
                .tuples()
                .filter_map(|(id, url, pg, ch_id)| {
                    Some(PageTable {
                        chapter_id: ch_id.to_string(),
                        url: url.to_string(),
                        page_number: str::parse(pg).ok()?,
                        id: str::parse(id).ok()?,
                    })
                })
                .collect(),
        })
        .collect())
}
