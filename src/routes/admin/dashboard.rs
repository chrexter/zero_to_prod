//! src/routes/admin/dashboard.rs
use actix_web::{
    error::ErrorInternalServerError,
    http::header::{ContentType, LOCATION},
    web::Data,
    HttpResponse,
};
use anyhow::Context;
use sqlx::PgPool;
use std::fmt::{Debug, Display};
use uuid::Uuid;

use crate::session_state::TypedSession;

fn error_500<T>(error: T) -> actix_web::Error
where
    T: Debug + Display + 'static,
{
    ErrorInternalServerError(error)
}

pub async fn admin_dashboard(
    session: TypedSession,
    pool: Data<PgPool>,
) -> Result<HttpResponse, actix_web::Error> {
    let username = if let Some(user_id) = session.get_user_id().map_err(error_500)? {
        get_username(user_id, &pool).await.map_err(error_500)?
    } else {
        return Ok(HttpResponse::SeeOther()
            .insert_header((LOCATION, "/login"))
            .finish());
    };

    Ok(HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(format!(
            r#"
                <!DOCTYPE html>
                <html lang="en">
                <head>
                    <meta http-equiv="content-type" content="text/html; charset=utf-8">
                    <title>Admin dashboard</title>
                    </head>
                <body>
                    <p>Welcome {username}!</p>
                </body>
                </html>
            "#
        )))
}

#[tracing::instrument(name = "Get username", skip(pool))]
async fn get_username(user_id: Uuid, pool: &PgPool) -> Result<String, anyhow::Error> {
    let row = sqlx::query!(
        r#"
        SELECT username
        FROM users
        WHERE user_id = $1
        "#,
        user_id,
    )
    .fetch_one(pool)
    .await
    .context("Failed to perform a query to retrieve a username.")?;

    Ok(row.username)
}