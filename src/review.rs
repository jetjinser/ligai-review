use flowsnet_platform_sdk::write_error_log;
use openai_flows::{
    chat::{ChatModel, ChatOptions},
    FlowsAccount, OpenAIFlows,
};

use crate::CHAR_SOFT_LIMIT;

pub async fn get_review(
    title: &str,
    pull_number: u64,
    patch: String,
    account: FlowsAccount,
) -> Option<String> {
    let mut current_commit = String::new();
    let mut commits: Vec<String> = Vec::new();
    for line in patch.lines() {
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
    of.set_flows_account(account);
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
