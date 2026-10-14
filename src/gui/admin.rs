use super::*;
use anyhow::{Context, Result, ensure};
use lttl::identity::{self, admin::{self as model, Activity, Change, Lifecycle, Target}};
use serde_json::Value;

pub struct Request { path: String, target: Option<Target>, before: Option<i64>, action: Option<Value> }
pub struct Snapshot { target: Target, lifecycle: Lifecycle, activity: Option<Activity>, before: Option<i64> }
pub struct Outcome { snapshot: Snapshot, message: String }

impl Request {
    pub fn run(self, profile: &identity::Profile, cancel: &AtomicBool, notice: &mut dyn FnMut(String)) -> Result<Outcome> {
        let target=Target::read(&self.path,profile)?;
        if let Some(expected)=self.target { ensure!(target==expected,"Archive selection changed. Refresh before making changes."); }
        let session=identity::login(profile,None,cancel,notice)?;
        ensure!(!cancel.load(Ordering::Relaxed),"Administration cancelled before submission");
        let value=session.retention(&target.archive_id,self.action.as_ref()).context(
            "Administrator request failed. Check the configured audit administrator role. If saving a change, its outcome may be unknown: refresh before retrying. Retention cannot be shortened and destroyed keys cannot be changed.")?;
        let lifecycle:Lifecycle=serde_json::from_value(value).context("The service replied but its settings could not be read. A submitted change may already be saved; refresh before retrying.")?;
        ensure!(lifecycle.archive_id==target.archive_id,"Unexpected archive in service response");
        let activity=session.audit_activity(&target.archive_id,self.before).and_then(|page|{
            ensure!(page.events.len()<=10 && page.events.iter().all(|e|e.record.archive==target.archive_id),"Unexpected activity response");Ok(page)
        });
        let (activity,warning)=match activity {
            Ok(page)=>(Some(page),String::new()),
            Err(e)=>(None,format!(" Activity could not be loaded: {e:#}. Check broker activity support and audit health, then refresh.")),
        };
        let message=format!("{} as {}.{}",if self.action.is_some(){"Change saved"}else{"Administration loaded"},session.user.name,warning);
        Ok(Outcome{snapshot:Snapshot{target,lifecycle,activity,before:self.before},message})
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Tab { Activity, Retention, Holds }
struct Pending { action: Value, description: String, target: Target }
pub struct State {
    pub path: String,
    loaded: Option<Snapshot>, tab: Tab, reason: String,
    retain_date: String, retain_forever: bool, expires_date: String, no_expiry: bool,
    pending: Option<Pending>, validation: Option<String>,
}
impl Default for State {
    fn default() -> Self { Self {path:String::new(),loaded:None,tab:Tab::Activity,reason:String::new(),
        retain_date:String::new(),retain_forever:false,expires_date:String::new(),no_expiry:true,pending:None,validation:None} }
}
impl State {
    pub fn accept(&mut self, result: Outcome) -> String {
        let state=&result.snapshot.lifecycle;
        self.retain_forever=state.retain_until==0;
        self.retain_date=model::date_text(if self.retain_forever {model::now()+86400*365}else{state.retain_until});
        self.no_expiry=state.access_expires==0;
        self.expires_date=model::date_text(if self.no_expiry {model::now()+86400*30}else{state.access_expires});
        self.reason.clear();self.pending=None;self.validation=None;self.loaded=Some(result.snapshot);
        result.message
    }
    pub fn clear(&mut self) { self.loaded=None;self.pending=None;self.validation=None;self.reason.clear(); }
    fn review(&mut self, change: Change, hold_id: &str) {
        let Some(snapshot)=&self.loaded else{return};
        let (date,unlimited)=if change==Change::Retention {(&self.retain_date,self.retain_forever)}else{(&self.expires_date,self.no_expiry)};
        match model::action(change,&snapshot.lifecycle,date,unlimited,hold_id,&self.reason) {
            Ok(action)=>{
                let description=match change {
                    Change::Retention=>format!("Retain until: {}",model::deadline(action["retain_until"].as_i64().unwrap(),"Indefinitely")),
                    Change::AccessExpiry=>{
                        let expires=action["expires"].as_i64().unwrap();
                        format!("Access expires: {}.{}",model::deadline(expires,"No expiry"),if expires!=0 && expires<=model::now(){" Access will be blocked immediately."}else{" Preservation is unchanged."})
                    },
                    Change::PlaceHold=>"Place a legal hold. Keys and evidence remain preserved until every active hold is released.".into(),
                    Change::ReleaseHold=>format!("Release hold {hold_id}. If no other hold remains, expired retention may permit disposition. This does not restore access or remove the external vault's holds."),
                };
                self.pending=Some(Pending{action,description,target:snapshot.target.clone()});self.validation=None;
            },
            Err(e)=>self.validation=Some(e.to_string()),
        }
    }
    pub fn ui(&mut self, ui: &mut egui::Ui, profile: &identity::Profile) -> Option<Request> {
        if self.loaded.as_ref().is_some_and(|s|!s.target.matches(self.path.trim(),profile)) { self.clear(); }
        section_heading(ui,"Audit & retention","Manage a protected archive with your organization's administrator account.");
        ui.label(format!("Company service: {}",if profile.broker_url.is_empty(){"Configure Identity & sharing first"}else{&profile.broker_url}));
        let mut changed=false;
        ui.horizontal(|ui|{
            changed=ui.add(egui::TextEdit::singleline(&mut self.path).hint_text("Choose a protected .lttl archive").desired_width((ui.available_width()-100.0).max(100.0))).changed();
            if ui.button("Browse…").clicked() {if let Some(path)=rfd::FileDialog::new().add_filter("Protected archive",&["lttl"]).pick_file(){self.path=path.display().to_string();changed=true;}}
        });
        if changed {self.clear();}
        let mut request=None;
        if ui.add_enabled(!self.path.trim().is_empty() && !profile.broker_url.is_empty(),primary_button("Sign in & refresh")).clicked() {
            request=Some(Request{path:self.path.trim().into(),target:None,before:None,action:None});
        }
        ui.label(RichText::new("Viewing and changing settings requires fresh organization sign-in. No archive contents are decrypted.").small().color(MUTED));
        let Some(snapshot)=&self.loaded else {ui.label("Select an archive and sign in to load its settings and activity. Your account must be configured as an audit administrator on the company service.");return request;};
        ui.separator();
        ui.label(RichText::new("ARCHIVE ID").small().color(MUTED));ui.monospace(&snapshot.target.archive_id);
        ui.label(RichText::new("Selected from the file header; verify this ID against your organization's records before changing settings.").small().color(MUTED));
        ui.horizontal_wrapped(|ui|{
            ui.label(format!("Retained: {}",model::deadline(snapshot.lifecycle.retain_until,"Indefinitely")));
            ui.label(format!("Access expiry: {}",model::deadline(snapshot.lifecycle.access_expires,"None")));
            ui.label(format!("Active holds: {}",snapshot.lifecycle.holds.len()));
        });
        if snapshot.lifecycle.destroyed {ui.colored_label(Color32::LIGHT_RED,"Archive key destroyed — settings are read-only.");}
        ui.horizontal(|ui|{ui.selectable_value(&mut self.tab,Tab::Activity,"Activity");ui.selectable_value(&mut self.tab,Tab::Retention,"Retention & access");ui.selectable_value(&mut self.tab,Tab::Holds,"Legal holds");});
        ui.separator();
        match self.tab {
            Tab::Activity=>{
                ui.label("Archive activity · newest first · UTC");
                ui.label(RichText::new("Service-verified signatures. Export status reflects the broker's vault receipt. Full independent evidence remains in the security team's vault; expired local history may be absent.").small().color(MUTED));
                if let Some(page)=&snapshot.activity {
                    if page.events.is_empty(){ui.label("No locally retained activity on this page.");}
                    for event in &page.events {
                        egui::Frame::group(ui.style()).show(ui,|ui|{
                            ui.label(RichText::new(format!("{} · {}",model::date_text(event.record.at),event.record.event.replace(['-','_']," "))).strong());
                            let actor=event.record.actor.as_deref().unwrap_or("System / unauthenticated");
                            ui.label(format!("By: {}",event.actor_name.as_deref().unwrap_or(actor)));
                            ui.collapsing(format!("Record #{} · {}",event.record.seq,if event.exported{"Vault acknowledged"}else{"Export pending"}),|ui|{
                                ui.label(format!("Actor ID: {actor}"));
                                if event.actor_name.is_some(){ui.small("Display name is from the current directory; actor ID is recorded in the event.");}
                                ui.add(egui::Label::new(RichText::new(serde_json::to_string_pretty(&event.record.details).unwrap_or_default()).monospace()).wrap());
                            });
                        });
                    }
                    ui.horizontal(|ui|{
                        if ui.add_enabled(snapshot.before.is_some(),egui::Button::new("Latest activity")).clicked(){request=Some(Request{path:self.path.trim().into(),target:Some(snapshot.target.clone()),before:None,action:None});}
                        if ui.add_enabled(page.next_before.is_some(),egui::Button::new("Older activity")).clicked(){request=Some(Request{path:self.path.trim().into(),target:Some(snapshot.target.clone()),before:page.next_before,action:None});}
                    });
                } else {ui.colored_label(Color32::LIGHT_RED,"Activity unavailable. Check the status message, update the broker if needed, and refresh.");}
            },
            Tab::Retention | Tab::Holds=>{
                let destroyed=snapshot.lifecycle.destroyed;
                let holds=snapshot.lifecycle.holds.clone();
                let mut review=None;
                ui.add_enabled_ui(!destroyed && self.pending.is_none(),|ui|{
                    ui.label("Reason for this change (required; include a case or approval reference)");
                    ui.add(egui::TextEdit::multiline(&mut self.reason).desired_rows(2).desired_width(f32::INFINITY));
                    ui.separator();
                    if self.tab==Tab::Retention {
                        ui.label("All dates are UTC: YYYY-MM-DD HH:MM (optional :SS).");
                        ui.label(RichText::new("Preservation deadline").strong());
                        ui.checkbox(&mut self.retain_forever,"Retain indefinitely");
                        ui.add_enabled(!self.retain_forever,egui::TextEdit::singleline(&mut self.retain_date));
                        ui.label("Retention can only be extended. Expiry of retention does not automatically delete the archive or its key.");
                        if ui.button("Review retention change").clicked(){review=Some((Change::Retention,String::new()));}
                        ui.separator();
                        ui.label(RichText::new("Access deadline").strong());
                        ui.checkbox(&mut self.no_expiry,"No access expiry");
                        ui.add_enabled(!self.no_expiry,egui::TextEdit::singleline(&mut self.expires_date));
                        ui.label("Access expiry blocks new key releases while preserving retained evidence and keys. Recipient permissions and revocation still apply.");
                        if ui.button("Review access expiry").clicked(){review=Some((Change::AccessExpiry,String::new()));}
                    } else {
                        ui.label("Holds preserve broker keys and evidence, independently of access expiry. Keep archive files in managed storage with matching holds.");
                        if ui.button("Place legal hold…").clicked(){review=Some((Change::PlaceHold,String::new()));}
                        if holds.is_empty(){ui.label("No active legal holds.");}
                        for hold in &holds {
                            egui::Frame::group(ui.style()).show(ui,|ui|{
                                ui.label(RichText::new(&hold.reason).strong());ui.monospace(&hold.id);
                                ui.label(format!("Placed {} UTC · by {}",model::date_text(hold.created),hold.actor));
                                if ui.button("Release this hold…").clicked(){review=Some((Change::ReleaseHold,hold.id.clone()));}
                            });
                        }
                    }
                });
                if let Some((change,id))=review {self.review(change,&id);}
            },
        }
        if let Some(error)=&self.validation {ui.colored_label(Color32::LIGHT_RED,error);}
        let mut confirm=false;let mut cancel=false;
        if let Some(pending)=&self.pending {
            egui::Frame::group(ui.style()).show(ui,|ui|{
                ui.label(RichText::new("Review change").strong());
                ui.label(&pending.description);ui.label(format!("Archive: {}",pending.target.archive_id));
                ui.label(format!("Service: {}",pending.target.broker));
                ui.label(format!("Reason: {}",pending.action["reason"].as_str().unwrap_or_default()));
                ui.horizontal(|ui|{confirm=ui.button("Sign in & apply change").clicked();cancel=ui.button("Cancel change").clicked();});
            });
        }
        if confirm {if let Some(pending)=self.pending.take(){request=Some(Request{path:self.path.trim().into(),target:Some(pending.target),before:None,action:Some(pending.action)});}}
        if cancel {self.pending=None;}
        request
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn loaded() -> (State,identity::Profile) {
        let profile=identity::Profile{broker_url:"https://keys.example".into(),..Default::default()};
        let id=identity::crypto::identifier();
        let mut state=State{path:"archive.lttl".into(),..Default::default()};
        state.accept(Outcome{snapshot:Snapshot{
            target:Target{archive_id:id.clone(),broker:profile.broker_url.clone(),path:state.path.clone()},
            lifecycle:Lifecycle{archive_id:id,retain_until:model::now()+86400,access_expires:0,destroyed:false,
                holds:vec![model::Hold{id:identity::crypto::identifier(),reason:"Case 42".into(),actor:"admin".into(),created:model::now()}]},
            activity:Some(Activity{events:vec![],next_before:None}),before:None},message:"Loaded".into()});
        (state,profile)
    }
    fn frame(state:&mut State,profile:&identity::Profile,ctx:&egui::Context,events:Vec<egui::Event>)->(egui::FullOutput,Option<Request>) {
        let mut request=None;
        let output=ctx.run(egui::RawInput{screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(580.0,1000.0))),events,..Default::default()},|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{request=state.ui(ui,profile);});
        });
        (output,request)
    }
    fn text_rect(output:&egui::FullOutput,label:&str)->egui::Rect {
        fn find(shape:&egui::Shape,label:&str)->Option<egui::Rect>{
            match shape {
                egui::Shape::Text(text) if text.galley.text()==label=>Some(egui::Rect::from_min_size(text.pos,text.galley.size())),
                egui::Shape::Vec(shapes)=>shapes.iter().find_map(|s|find(s,label)), _=>None,
            }
        }
        output.shapes.iter().find_map(|s|find(&s.shape,label)).unwrap_or_else(||panic!("Missing UI label: {label}"))
    }
    fn click(state:&mut State,profile:&identity::Profile,ctx:&egui::Context,label:&str)->Option<Request>{
        // A settling frame is needed after changing tabs so egui has widget geometry.
        frame(state,profile,ctx,vec![]);
        let (output,_)=frame(state,profile,ctx,vec![]);let pos=text_rect(&output,label).center();
        let events=vec![egui::Event::PointerMoved(pos),egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:true,modifiers:egui::Modifiers::NONE},egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:false,modifiers:egui::Modifiers::NONE}];
        frame(state,profile,ctx,events).1
    }
    #[test]
    fn hold_release_requires_review_and_confirmed_target() {
        let (mut state,profile)=loaded();let ctx=egui::Context::default();
        state.tab=Tab::Holds;state.reason="Case closed".into();
        let expected=state.loaded.as_ref().unwrap().target.clone();
        let hold=state.loaded.as_ref().unwrap().lifecycle.holds[0].id.clone();
        assert!(click(&mut state,&profile,&ctx,"Release this hold…").is_none());
        assert!(state.pending.is_some());
        let request=click(&mut state,&profile,&ctx,"Sign in & apply change").expect("confirmation emits request");
        assert_eq!(request.target.unwrap(),expected);
        let action=request.action.unwrap();assert_eq!(action["action"],"release_hold");assert_eq!(action["hold_id"],hold);assert_eq!(action["reason"],"Case closed");
    }
    #[test]
    fn changing_service_or_archive_discards_pending_change() {
        for change_service in [false,true] {
            let (mut state,mut profile)=loaded();state.reason="case".into();state.review(Change::PlaceHold,"");
            if change_service {profile.broker_url="https://other.example".into();}else{state.path="other.lttl".into();}
            assert!(frame(&mut state,&profile,&egui::Context::default(),vec![]).1.is_none());
            assert!(state.pending.is_none());assert!(state.loaded.is_none());
        }
    }
    #[test]
    fn retention_and_expiry_review_validation_and_cancel() {
        let (mut state,profile)=loaded();let ctx=egui::Context::default();state.tab=Tab::Retention;
        assert!(click(&mut state,&profile,&ctx,"Review retention change").is_none());
        assert!(state.validation.is_some());assert!(state.pending.is_none());
        state.reason="Preserve case".into();state.retain_forever=true;
        click(&mut state,&profile,&ctx,"Review retention change");
        assert_eq!(state.pending.as_ref().unwrap().action["retain_until"],0);
        click(&mut state,&profile,&ctx,"Cancel change");assert!(state.pending.is_none());
        state.no_expiry=false;state.expires_date=model::date_text(model::now()-60);
        click(&mut state,&profile,&ctx,"Review access expiry");
        assert!(state.pending.as_ref().unwrap().description.contains("blocked immediately"));
    }
    #[test]
    fn activity_pagination_and_destroyed_archive_state() {
        let (mut state,profile)=loaded();let ctx=egui::Context::default();
        state.loaded.as_mut().unwrap().activity.as_mut().unwrap().next_before=Some(120);
        let request=click(&mut state,&profile,&ctx,"Older activity").unwrap();assert_eq!(request.before,Some(120));assert!(request.action.is_none());
        state.loaded.as_mut().unwrap().lifecycle.destroyed=true;state.reason="case".into();state.review(Change::PlaceHold,"");
        assert!(state.pending.is_none());assert!(state.validation.is_some());
    }
}
