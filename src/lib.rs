use std::env;

use github_flows::{get_octo, listen_to_event, EventPayload, GithubLogin};
use ligab::Liga;
use regex::Regex;
use serde_json::Value;

#[no_mangle]
#[tokio::main(flavor = "current_thread")]
pub async fn run() {
    let login = GithubLogin::Default;
    let owner = env::var("owner").unwrap_or("jetjinser".to_string());
    let repo = env::var("repo").unwrap_or("fot".to_string());
    let events = vec!["issue_comment"];

    let token = env::var("LIGA_TOKEN");
    let client_id = env::var("client_id");
    let secret_key = env::var("secret_key");

    let liga = if let Ok(t) = token {
        Liga::from_token(t)
    } else {
        let client_id = client_id.unwrap();
        let secret_key = secret_key.unwrap();
        Liga::from_client(client_id, secret_key)
    };

    listen_to_event(&login, &owner, &repo, events, |payload| async {
        handle(&login, &owner, &repo, liga, payload).await
    })
    .await;
}

async fn handle(login: &GithubLogin, owner: &str, repo: &str, liga: Liga, payload: EventPayload) {
    if let EventPayload::IssueCommentEvent(e) = payload {
        let comment = e.comment.body.unwrap_or_default();

        if !comment.trim_start().starts_with("liga") {
            return;
        }

        let octo = get_octo(login);

        let re = Regex::new(r#"liga#(\w{1,5}-\d)"#).unwrap();
        let captures = re.captures(&comment);
        let issue_number = if let Some(cap) = captures {
            cap.get(1).unwrap().as_str()
        } else {
            return;
        };

        let issue: Value = liga.issue().get_by_issue_number(issue_number);

        let code = issue["code"].as_i64().unwrap_or(-1);
        if code != 0 {
            // Error here
            return;
        }

        let data = &issue["data"];
        let project_id = &data["projectId"];
        let issue_id = data["id"].as_u64().unwrap() as u32;
        let description = data["data"]["description"].as_str().unwrap_or_default();

        // TODO: review
        let body = comment;
        let data = serde_json::json!({
            "description": format!("{}\n{}", description, body),
        });
        let res: Value = liga.issue().update(data, issue_id);

        let id = &res["data"]["id"].as_u64();
        let number = e.issue.number;
        if let Some(i) = id {
            let url = format!("https://ligai.cn/app/work/table?pid={project_id}&issueid={i}");
            let body = format!(
                "You just created issue: {}\nplease visit {} to check it.",
                i, url
            );
            _ = octo.issues(owner, repo).create_comment(number, body).await;
        } else {
            _ = octo
                .issues(owner, repo)
                .create_comment(
                    number,
                    format!("failed...\n{:?}", serde_json::to_string(&res)),
                )
                .await;
        }
    }
}
