//! src/routes/subscriptions

use actix_web::web::{Data, Form};
use actix_web::{HttpResponse, ResponseError};
use chrono::Utc;
use sqlx::{Executor, PgPool, Postgres, Transaction};
use uuid::Uuid;

use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};

use crate::domain::NewSubscriber;
use crate::email_client::EmailClient;
use crate::startup::ApplicationBaseUrl;

#[derive(serde::Deserialize)]
pub struct FormData {
    pub email: String,
    pub name: String,
}

#[tracing::instrument(
    name = "Adding a new subscriber",
    skip(form, pool, email_client, base_url),
    fields(
        subscriber_email = %form.email,
        subscriber_name = %form.name
    )
)]
pub async fn subscribe(
    form: Form<FormData>,
    pool: Data<PgPool>,
    email_client: Data<EmailClient>,
    base_url: Data<ApplicationBaseUrl>,
) -> Result<HttpResponse, actix_web::Error> {
    let new_subscriber = match form.0.try_into() {
        Ok(subsciber) => subsciber,
        Err(_) => return Ok(HttpResponse::BadRequest().finish()),
    };

    let mut transaction = match pool.begin().await {
        Ok(transaction) => transaction,
        Err(_) => return Ok(HttpResponse::InternalServerError().finish()),
    };

    let subscriber_id = match insert_subscriber(&mut transaction, &new_subscriber).await {
        Ok(subscriber_id) => subscriber_id,
        Err(_) => return Ok(HttpResponse::InternalServerError().finish()),
    };

    let subscription_token = ganerate_subscription_token();
    store_subscription_token(&mut transaction, subscriber_id, &subscription_token).await?;

    if transaction.commit().await.is_err() {
        return Ok(HttpResponse::InternalServerError().finish());
    }

    let sent_email = send_confirmation_email(
        &email_client,
        new_subscriber,
        &base_url.0,
        &subscription_token,
    );
    if sent_email.await.is_err() {
        return Ok(HttpResponse::InternalServerError().finish());
    }

    Ok(HttpResponse::Ok().finish())
}

#[tracing::instrument(
    name = "Send a confirmation email to a new subscriber",
    skip(email_client, new_subscriber, base_url, subscription_token)
)]
async fn send_confirmation_email(
    email_client: &EmailClient,
    new_subscriber: NewSubscriber,
    base_url: &str,
    subscription_token: &str,
) -> Result<(), reqwest::Error> {
    let confirmation_link = format!(
        "{}/subscriptions/confirm?subscription_token={}",
        base_url, subscription_token
    );

    let html_email = format!(
        "Welcome to our newsletter!<br />\
        Click <a href=\"{}\">here</a> to confirm your subscription.",
        confirmation_link
    );

    let text_email = format!(
        "Welcome to our newsletter!\nVisit {} to confirm your subscription.",
        confirmation_link
    );

    email_client
        .send_email(new_subscriber.email, "Welcome!", &html_email, &text_email)
        .await
}

#[tracing::instrument(
    name = "Saving new subscriber details in the database",
    skip(transaction, subscriber)
)]
async fn insert_subscriber(
    transaction: &mut Transaction<'static, Postgres>,
    subscriber: &NewSubscriber,
) -> Result<Uuid, sqlx::Error> {
    let subscriber_id = Uuid::new_v4();

    let query = sqlx::query!(
        r#"
        INSERT INTO subscriptions (id, email, name, subscribed_at, status)
        VALUES ($1, $2, $3, $4, 'pending_confirmation')
        "#,
        subscriber_id,
        subscriber.email.as_ref(),
        subscriber.name.as_ref(),
        Utc::now()
    );

    transaction.execute(query).await.map_err(|error| {
        tracing::error!("Failed to execute query: {:?}", error);
        error
    })?;

    Ok(subscriber_id)
}

#[tracing::instrument(name = "Generating subscription token")]
fn ganerate_subscription_token() -> String {
    let mut rng = thread_rng();

    std::iter::repeat_with(|| rng.sample(Alphanumeric))
        .map(char::from)
        .take(25)
        .collect()
}

#[tracing::instrument(
    name = "Store subscription token in the database",
    skip(subscription_token, transaction)
)]
pub async fn store_subscription_token(
    transaction: &mut Transaction<'static, Postgres>,
    subscriber_id: Uuid,
    subscription_token: &str,
) -> Result<(), StoreTokenError> {
    let query = sqlx::query!(
        r#"
        INSERT INTO subscription_tokens (subscription_token, subscriber_id)
        VALUES ($1, $2)
        "#,
        subscription_token,
        subscriber_id
    );

    transaction.execute(query).await.map_err(|error| {
        tracing::error!("Failed to execute query: {:?}", error);
        StoreTokenError(error)
    })?;

    Ok(())
}

#[derive(Debug)]
pub struct StoreTokenError(sqlx::Error);

impl ResponseError for StoreTokenError {}

impl std::fmt::Display for StoreTokenError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "A database error was encountered while \
            trying to store a subscription token."
        )
    }
}
