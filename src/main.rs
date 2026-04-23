use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use cyper::Client;
use frankenstein::{
    AsyncTelegramApi,
    input_file::FileUpload,
    methods::{GetStickerSetParams, GetUpdatesParams, SendStickerParams},
    types::ReplyParameters,
    updates::UpdateContent,
};
use rand::{rng, seq::IndexedRandom};

struct Bot {
    api_url: String,
    client: Client,
}

impl AsyncTelegramApi for Bot {
    type Error = cyper::Error;

    async fn request<Params, Output>(
        &self,
        method: &str,
        params: Option<Params>,
    ) -> Result<Output, Self::Error>
    where
        Params: serde::ser::Serialize + std::fmt::Debug + std::marker::Send,
        Output: serde::de::DeserializeOwned,
    {
        let url = format!("{}/{method}", self.api_url);
        let mut req = self
            .client
            .post(url)?
            .header("Content-Type", "application/json")?;
        if let Some(param) = params {
            req = req.json(&param)?;
        }
        req.send().await?.json().await
    }

    async fn request_with_form_data<Params, Output>(
        &self,
        _: &str,
        _: Params,
        _: Vec<(&str, std::path::PathBuf)>,
    ) -> Result<Output, Self::Error>
    where
        Params: serde::ser::Serialize + std::fmt::Debug + std::marker::Send,
        Output: serde::de::DeserializeOwned,
    {
        todo!("Not supported nor used")
    }
}

fn curr_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Bruh what year is it")
        .as_secs()
}

#[compio::main]
async fn main() {
    let api_token =
        std::env::var("BOT_TOKEN").expect("Failed to load bot token. Set BOT_TOKEN env var");

    let root_store = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let client = Client::builder()
        .use_rustls(Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth(),
        ))
        .build();
    let bot = Bot {
        api_url: format!("https://api.telegram.org/bot{}", api_token),
        client,
    };

    // Get the sticker set
    let sticker_set_name = "qrb_by_fStikBot";
    let sticker_set_params = GetStickerSetParams::builder()
        .name(sticker_set_name)
        .build();

    let sticker_set = match bot.get_sticker_set(&sticker_set_params).await {
        Ok(response) => response.result,
        Err(e) => {
            eprintln!("Failed to get sticker set: {:?}", e);
            return;
        }
    };

    let stickers = sticker_set
        .stickers
        .into_iter()
        .map(|s| s.file_id)
        .collect::<Vec<_>>();
    eprintln!(
        "Loaded {} stickers from set '{}'",
        stickers.len(),
        sticker_set_name
    );

    // Start polling for updates
    let mut offset: Option<i64> = None;
    let mut rng = rng();

    loop {
        let params = GetUpdatesParams::builder().timeout(30u32);
        let params = if let Some(offset) = offset {
            params.offset(offset).build()
        } else {
            params.build()
        };

        let updates = match bot.get_updates(&params).await {
            Ok(response) => response.result,
            Err(e) => {
                eprintln!("Failed to get updates: {:?}", e);
                compio::time::sleep(std::time::Duration::from_secs(1)).await;
                continue;
            }
        };

        for update in updates {
            offset = Some(update.update_id as i64 + 1);

            let UpdateContent::Message(message) = update.content else {
                continue;
            };

            let Some(mut text) = message.text else {
                eprintln!("No text in the message, skip");
                continue;
            };

            if message.date.abs_diff(curr_time()) >= 60 {
                eprintln!("Recevied stale message, skip");
                continue;
            }

            text.make_ascii_lowercase();

            // Check if message contains keywords
            if text.contains("qrb") || text.contains("去日本") {
                // Select a random sticker
                let sticker_id = stickers
                    .choose(&mut rng)
                    .expect("The sticker set should not be empty");

                // Send the sticker
                let send_params = SendStickerParams::builder()
                    .chat_id(message.chat.id)
                    .reply_parameters(
                        ReplyParameters::builder()
                            .message_id(message.message_id)
                            .allow_sending_without_reply(true)
                            .build(),
                    )
                    .sticker(FileUpload::String(sticker_id.clone()))
                    .build();

                if let Err(e) = bot.send_sticker(&send_params).await {
                    eprintln!("Failed to send sticker: {:?}", e);
                } else {
                    eprintln!(
                        "Sent sticker to chat {} in reply to #{}",
                        message.chat.id, message.message_id
                    );
                }
            }
        }
    }
}
