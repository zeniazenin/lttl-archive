//! Hosting is independent of the user's identity provider.
use serde::{Deserialize,Serialize};

#[derive(Clone,Copy,Debug,Default,PartialEq,Eq,Serialize,Deserialize,clap::ValueEnum)]
#[serde(rename_all="kebab-case")]
pub enum Hosting {#[default] Azure,Aws,Gcp,SelfHosted}
impl Hosting {
    pub const ALL:[Self;4]=[Self::Azure,Self::Aws,Self::Gcp,Self::SelfHosted];
    pub fn label(self)->&'static str {match self {Self::Azure=>"Azure",Self::Aws=>"AWS",Self::Gcp=>"Google Cloud",Self::SelfHosted=>"Self-hosted"}}
    pub fn instructions(self)->&'static str {match self {
        Self::Azure=>include_str!("../../identity-broker/deploy/azure.md"),
        Self::Aws=>include_str!("../../identity-broker/deploy/aws.md"),
        Self::Gcp=>include_str!("../../identity-broker/deploy/gcp.md"),
        Self::SelfHosted=>include_str!("../../identity-broker/deploy/self-hosted.md"),
    }}
}
pub fn generate(provider:super::guides::Provider,hosting:Hosting,broker:&str,tenant:&str,client_id:&str)->anyhow::Result<String> {
    let mut text=super::guides::generate(provider,broker,tenant,client_id)?;
    text.push_str("\n---\n\n");text.push_str(hosting.instructions());
    text.push_str("\n\n");text.push_str(include_str!("../../identity-broker/deploy/common.md"));Ok(text)
}
