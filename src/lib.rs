use flowsnet_platform_sdk::write_error_log;
use openai_flows::{
    chat::{ChatModel, ChatOptions},
    OpenAIFlows,
};
use std::env;

use github_flows::{get_octo, listen_to_event, octocrab::Octocrab, EventPayload, GithubLogin};
use ligab::Liga;
use regex::Regex;
use serde_json::Value;

//  The soft character limit of the input context size
//   the max token size or word count for GPT4 is 8192
//   the max token size or word count for GPT35Turbo is 4096
static CHAR_SOFT_LIMIT: usize = 9000;

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
        if e.issue.pull_request.is_none() {
            write_error_log!("not pr");
            return;
        }

        let comment = e.comment.body.unwrap_or_default();
        let octo = get_octo(login);

        let re = Regex::new(r#"liga#(\w{1,5}-\d)"#).unwrap();
        let captures = re.captures(&comment);

        let issue_number = if let Some(cap) = captures {
            cap.get(1).unwrap().as_str()
        } else {
            return;
        };

        let issue: Value = liga.issue().get_by_issue_number(issue_number);

        let code = issue["code"].as_str().unwrap_or("-1");
        if code != "0" {
            // Error here
            return;
        }

        let data = &issue["data"];
        let project_id = &data["projectId"];
        let issue_id = data["id"].as_u64().unwrap() as u32;
        let description = data["data"]["description"].as_str().unwrap_or_default();

        let title = e.issue.title;
        let pull_number = e.issue.number;
        let body = get_review(octo, owner, repo, &title, pull_number)
            .await
            .unwrap();

        let data = serde_json::json!({
            "description": format!("{}\n{}", description, body),
        });
        let res: Value = liga.issue().update(data, issue_id);

        let success = &res["data"]["success"].as_bool();
        let number = e.issue.number;

        if let Some(suc) = success {
            if *suc {
                let url =
                    format!("https://ligai.cn/app/work/table?pid={project_id}&issueid={issue_id}");
                let body = format!("Review sent!\nplease visit [ligai]({url}) to check it.");
                _ = octo.issues(owner, repo).create_comment(number, body).await;
            }

            return;
        }

        _ = octo
            .issues(owner, repo)
            .create_comment(
                number,
                format!("failed...\n{:?}", serde_json::to_string(&res)),
            )
            .await;
    }
}

async fn get_review(
    octo: &Octocrab,
    owner: &str,
    repo: &str,
    title: &str,
    pull_number: u64,
) -> Option<String> {
    let pulls = octo.pulls(owner, repo);

    let patch_as_text = pulls.get_patch(pull_number).await.unwrap();
    let mut current_commit = String::new();
    let mut commits: Vec<String> = Vec::new();
    for line in patch_as_text.lines() {
        if line.starts_with("From ") {
            // Detected a new commit
            if !current_commit.is_empty() {
                // Store the previous commit
                commits.push(current_commit.clone());
            }
            // Start a new commit
            current_commit.clear();
        }
        // Append the line to the current commit if the current commit is less than CHAR_SOFT_LIMIT
        if current_commit.len() < CHAR_SOFT_LIMIT {
            current_commit.push_str(line);
            current_commit.push('\n');
        }
    }
    if !current_commit.is_empty() {
        // Store the last commit
        commits.push(current_commit.clone());
    }

    if commits.is_empty() {
        write_error_log!("Cannot parse any commit from the patch file");
        return None;
    }

    let mut of = OpenAIFlows::new();
    of.set_retry_times(3);

    let chat_id = format!("PR#{pull_number}");
    let system = &format!("You are an experienced software developer. You will act as a reviewer for a GitHub Pull Request titled \"{}\".", title);
    let mut reviews: Vec<String> = Vec::new();
    let mut reviews_text = String::new();
    for (_i, commit) in commits.iter().enumerate() {
        let co = ChatOptions {
            model: ChatModel::GPT35Turbo,
            restart: true,
            system_prompt: Some(system),
        };
        let question = "The following is a GitHub patch. Please summarize the key changes and identify potential problems. Start with the most important findings.\n\n".to_string() + commit;
        if let Ok(r) = of.chat_completion(&chat_id, &question, &co).await {
            if reviews_text.len() < CHAR_SOFT_LIMIT {
                reviews_text.push_str("------\n");
                reviews_text.push_str(&r.choice);
                reviews_text.push('\n');
            }
            reviews.push(r.choice);
        }
    }

    let mut resp = String::new();
    resp.push_str("Hello, I am a [serverless review bot](https://github.com/flows-network/github-pr-summary/) on [flows.network](https://flows.network/). Here are my reviews of code commits in this PR.\n\n------\n\n");
    if reviews.len() > 1 {
        let co = ChatOptions {
            model: ChatModel::GPT35Turbo,
            restart: true,
            system_prompt: Some(system),
        };
        let question = "Here is a set of summaries for software source code patches. Each summary starts with a ------ line. Please write an overall summary considering all the individual summary. Please present the potential issues and errors first, following by the most important findings, in your summary.\n\n".to_string() + &reviews_text;
        if let Ok(r) = of.chat_completion(&chat_id, &question, &co).await {
            write_error_log!("Got the overall summary");
            resp.push_str(&r.choice);
            resp.push_str("\n\n## Details\n\n");
        }
    }
    for (i, review) in reviews.iter().enumerate() {
        resp.push_str(&format!("### Commit {}\n", i + 1));
        resp.push_str(review);
        resp.push_str("\n\n");
    }

    Some(resp)
}
