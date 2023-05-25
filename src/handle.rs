use std::matches;

use flowsnet_platform_sdk::write_error_log;
use github_flows::{
    get_octo, octocrab::models::events::payload::PullRequestEventAction, EventPayload, GithubLogin,
};
use ligab::Liga;
use openai_flows::FlowsAccount;
use regex::Regex;
use serde_json::Value;

use crate::review::get_review;

pub async fn handle(
    login: &GithubLogin,
    owner: &str,
    repo: &str,
    liga: Liga,
    account: FlowsAccount,
    payload: EventPayload,
) {
    if let EventPayload::IssueCommentEvent(e) = payload {
        let pr_url = if let Some(pr) = e.issue.pull_request {
            pr.html_url
        } else {
            write_error_log!("not pr");
            return;
        };

        let comment = e.comment.body.unwrap_or_default();
        let octo = get_octo(login);

        let re = Regex::new(r#"LigaAI#(\w{1,5}-\d+)"#).unwrap();
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
        let body = get_review(octo, owner, repo, &title, pull_number, account)
            .await
            .unwrap();

        let data = serde_json::json!({
            "description": format!("{}\n> ref: {}\n{}", description, pr_url, body),
        });
        let res: Value = liga.issue().update(data, issue_id);

        let success = &res["data"]["success"].as_bool();
        let number = e.issue.number;

        if let Some(suc) = success {
            if *suc {
                let url =
                    format!("https://ligai.cn/app/work/table?pid={project_id}&issueid={issue_id}");
                let body = format!("Review sent!\nPlease visit [LigaAI]({url}) to check it out.");
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
    } else if let EventPayload::PullRequestEvent(e) = payload {
        if !matches!(e.action, PullRequestEventAction::Synchronize) {
            write_error_log!("not synchronize action");
            return;
        }

        let number = e.number;
        let octo = get_octo(login);

        _ = octo
            .issues(owner, repo)
            .create_comment(number, format!("```\n{:#?}\n```", e))
            .await;

        if let Some(rp) = e.pull_request.repo {
            if let Some(cp_url) = rp.compare_url {
                _ = octo
                    .issues(owner, repo)
                    .create_comment(number, format!("compare_url: {}", cp_url))
                    .await;
            } else {
                write_error_log!("no compare_url");
            }
        } else {
            write_error_log!("no repo");
        }
    }
}
