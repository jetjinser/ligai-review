use crate::handle::handle;
use openai_flows::FlowsAccount;
use std::env;

use github_flows::{listen_to_event, GithubLogin};
use ligab::Liga;

//  The soft character limit of the input context size
//   the max token size or word count for GPT4 is 8192
//   the max token size or word count for GPT35Turbo is 4096
static CHAR_SOFT_LIMIT: usize = 9000;

mod handle;
mod review;

#[no_mangle]
#[tokio::main(flavor = "current_thread")]
pub async fn run() {
    let login = GithubLogin::Default;
    let owner = env::var("owner").unwrap_or("jetjinser".to_string());
    let repo = env::var("repo").unwrap_or("fot".to_string());
    let events = vec!["issue_comment", "pull_request"];

    let token = env::var("LIGA_TOKEN");
    let client_id = env::var("client_id");
    let secret_key = env::var("secret_key");

    let account = match env::var("chat") {
        Ok(chat) => FlowsAccount::Provided(chat),
        Err(_) => FlowsAccount::Default,
    };

    let liga = if let Ok(t) = token {
        Liga::from_token(t)
    } else {
        let client_id = client_id.unwrap();
        let secret_key = secret_key.unwrap();
        Liga::from_client(client_id, secret_key)
    };

    listen_to_event(&login, &owner, &repo, events, |payload| async {
        handle(&login, &owner, &repo, liga, account, payload).await
    })
    .await;
}
