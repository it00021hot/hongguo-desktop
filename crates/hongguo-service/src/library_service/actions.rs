use super::{Kind, model::Entry};
use crate::AppState;
use hongguo_api::{client::ApiEnv, interact};
use hongguo_common::{AppError, AppResult};
use std::collections::{HashMap, HashSet};

pub struct Removal {
    pub removed: Vec<String>,
    pub errors: Vec<AppError>,
}

#[derive(Clone, Copy)]
pub enum RemovalMode {
    Row,
    Selection { online: bool },
}

pub async fn remove(
    state: AppState,
    kind: Kind,
    entries: Vec<Entry>,
    mode: RemovalMode,
) -> AppResult<Removal> {
    let settings = state.settings();
    if kind == Kind::Reservations {
        if settings.account.is_none() {
            return Err(AppError::Auth("未登录".into()));
        }
        return super::reservation_removal::remove(
            entries,
            mode,
            &state.api_env_from_settings(settings),
        )
        .await;
    }
    let user_id = settings
        .account
        .as_ref()
        .map(|account| account.user_id.clone());
    let env = state.api_env_from_settings(settings);
    if kind == Kind::Collections {
        let user_id = user_id
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(|| AppError::Auth("登录后可取消收藏".into()))?;
        return remove_collections(entries, &user_id, &env).await;
    }
    if kind == Kind::Liked {
        if user_id.is_none() {
            return Err(AppError::Auth("登录后可取消点赞".into()));
        }
        return super::likes_removal::remove(entries, &env).await;
    }
    Ok(remove_each(entries, move |entry| {
        let state = state.clone();
        async move {
            match kind {
                Kind::Follows => {
                    crate::follow_service::toggle(&state, &entry.series_id, false).await
                }
                _ => Err(AppError::InvalidArgs("该片库不支持逐项取消".into())),
            }
        }
    })
    .await)
}

async fn remove_collections(
    entries: Vec<Entry>,
    user_id: &str,
    env: &ApiEnv,
) -> AppResult<Removal> {
    if entries.is_empty() {
        return Ok(Removal {
            removed: Vec::new(),
            errors: Vec::new(),
        });
    }
    // Use the account's flat shelf, not the folder membership type or a guessed Watch value.
    let before = interact::fetch_bookshelf_strict(user_id, env).await?;
    let (targets, mut errors) = collection_targets(entries, &before);
    if targets.is_empty() {
        return Ok(Removal {
            removed: Vec::new(),
            errors,
        });
    }
    let mut seen = HashSet::new();
    let requests = targets
        .iter()
        .filter_map(|target| {
            seen.insert((&target.book_id, target.book_type)).then_some((
                target.book_id.clone(),
                target.book_type,
                false,
            ))
        })
        .collect::<Vec<_>>();
    interact::collect_series_batch(&requests, env).await?;
    // HTTP/JSON success alone does not prove the shelf changed.
    let after = interact::fetch_bookshelf_strict(user_id, env).await?;
    let remaining: HashSet<&str> = after
        .iter()
        .flat_map(|row| [row.book_id.as_str(), row.series_id.as_str()])
        .collect();
    let mut removed = Vec::new();
    for target in targets {
        if remaining.contains(target.book_id.as_str())
            || remaining.contains(target.series_id.as_str())
        {
            errors.push(AppError::Rejected("服务器未移除此收藏".into()));
        } else {
            removed.push(target.key);
        }
    }
    Ok(Removal { removed, errors })
}

struct CollectionRemovalTarget {
    key: String,
    series_id: String,
    book_id: String,
    book_type: i64,
}

fn index_id<'a>(
    index: &mut HashMap<&'a str, Option<&'a interact::BookshelfEntry>>,
    id: &'a str,
    row: &'a interact::BookshelfEntry,
) {
    index
        .entry(id)
        .and_modify(|value| *value = None)
        .or_insert(Some(row));
}

fn collection_targets(
    entries: Vec<Entry>,
    shelf: &[interact::BookshelfEntry],
) -> (Vec<CollectionRemovalTarget>, Vec<AppError>) {
    let mut targets = Vec::new();
    let mut errors = Vec::new();
    let mut books = HashMap::with_capacity(shelf.len());
    let mut aliases = HashMap::with_capacity(shelf.len());
    for row in shelf {
        index_id(&mut books, &row.book_id, row);
        index_id(&mut aliases, &row.book_id, row);
        if row.series_id != row.book_id {
            index_id(&mut aliases, &row.series_id, row);
        }
    }
    for entry in entries {
        let matched = match entry.collection_book_id.as_deref() {
            Some(id) => books.get(id),
            None => aliases.get(entry.series_id.as_str()),
        };
        if let Some(Some(row)) = matched
            && let Some(kind) = row.book_type
        {
            targets.push(CollectionRemovalTarget {
                key: entry.key,
                series_id: row.series_id.clone(),
                book_id: row.book_id.clone(),
                book_type: kind,
            });
        } else {
            errors.push(AppError::Rejected(
                "无法核对收藏条目的原始类型，未发送取消请求".into(),
            ));
        }
    }
    (targets, errors)
}

async fn remove_each<F, Fut>(entries: Vec<Entry>, mut remove: F) -> Removal
where
    F: FnMut(Entry) -> Fut,
    Fut: Future<Output = AppResult<()>>,
{
    let mut removed = Vec::new();
    let mut errors = Vec::new();
    for entry in entries {
        let key = entry.key.clone();
        let result = remove(entry).await;
        if let Err(error) = result {
            errors.push(error);
        } else {
            removed.push(key);
        }
    }
    Removal { removed, errors }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_selection_uses_original_ids_and_types_without_guessing() {
        let shelf = [
            interact::BookshelfEntry {
                series_id: "series-a".into(),
                book_id: "book-a".into(),
                book_type: Some(3),
                ..Default::default()
            },
            interact::BookshelfEntry {
                series_id: "series-b".into(),
                book_id: "book-b".into(),
                book_type: Some(4),
                ..Default::default()
            },
            interact::BookshelfEntry {
                series_id: "series-c".into(),
                book_id: "book-c".into(),
                ..Default::default()
            },
        ];
        let selected = ["series-a", "series-b", "series-c"].map(|id| Entry {
            key: id.into(),
            series_id: id.into(),
            ..Default::default()
        });
        let (targets, errors) = collection_targets(selected.into(), &shelf);
        let wire = targets
            .iter()
            .map(|target| (target.book_id.as_str(), target.book_type))
            .collect::<Vec<_>>();
        assert_eq!(wire, [("book-a", 3), ("book-b", 4)]);
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn bulk_collection_lookup_keeps_explicit_ids_and_rejects_ambiguous_aliases() {
        let mut shelf = (0..5_000)
            .map(|index| interact::BookshelfEntry {
                book_id: format!("book-{index}"),
                series_id: format!("series-{index}"),
                book_type: Some(2),
                ..Default::default()
            })
            .collect::<Vec<_>>();
        shelf[2].series_id = "book-1".into();
        let mut entries = shelf
            .iter()
            .map(|row| Entry {
                key: row.book_id.clone(),
                series_id: row.series_id.clone(),
                collection_book_id: Some(row.book_id.clone()),
                ..Default::default()
            })
            .collect::<Vec<_>>();
        entries.push(Entry {
            key: "ambiguous".into(),
            series_id: "book-1".into(),
            ..Default::default()
        });
        let (targets, errors) = collection_targets(entries, &shelf);
        assert_eq!(targets.len(), 5_000);
        assert_eq!(targets[1].book_id, "book-1");
        assert_eq!(errors.len(), 1);
    }
}
