let loadNumber = 0;
const branchAnimations=new WeakMap();
const reducedMotion=()=>window.matchMedia('(prefers-reduced-motion: reduce)').matches;
function stopBranchAnimation(node){
  branchAnimations.get(node)?.cancel();
  branchAnimations.delete(node);
  node.style.height='';node.style.overflow='';
}
function animateBranchHeight(node,from,to,closing=false){
  if(reducedMotion()||Math.abs(to-from)<2){
    stopBranchAnimation(node);
    if(closing){node.open=false;node.classList.remove('branch-closing');}
    return;
  }
  node.style.height=from+'px';node.style.overflow='clip';
  const animation=node.animate([{height:from+'px'},{height:to+'px'}],{
    duration:220,easing:'cubic-bezier(.2,.75,.25,1)'
  });
  branchAnimations.set(node,animation);
  animation.onfinish=()=>{
    if(branchAnimations.get(node)!==animation)return;
    stopBranchAnimation(node);
    if(closing){node.open=false;node.classList.remove('branch-closing');}
  };
}
// Opened content arrives softly instead of appearing at once.
function revealBranch(node){
  const body=node.querySelector(':scope > .object-body');
  if(!body||reducedMotion())return;
  body.animate([{opacity:0,transform:'translateY(-4px)'},{opacity:1,transform:'none'}],{duration:260,easing:'cubic-bezier(.2,.75,.25,1)'});
}
function closedBranchHeight(node){
  const style=getComputedStyle(node),summary=node.querySelector(':scope > summary');
  return summary.getBoundingClientRect().height+
    parseFloat(style.paddingTop)+parseFloat(style.paddingBottom)+
    parseFloat(style.borderTopWidth)+parseFloat(style.borderBottomWidth);
}
function enableBranchMotion(node){
  node.querySelector(':scope > summary').addEventListener('click',event=>{
    if(event.defaultPrevented||event.target.closest('button,a,input,label,select,textarea'))return;
    event.preventDefault();
    const opening=!node.open||node.classList.contains('branch-closing');
    // A branch with lazy content loads while closed, then opens once to its full height.
    if(opening&&!node.open&&node.branchPrepare&&!node.branchReady){
      if(node.classList.contains('is-loading'))return;
      node.classList.add('is-loading');
      node.branchPrepare().then(()=>{
        node.classList.remove('is-loading');node.branchReady=true;
        const start=node.getBoundingClientRect().height;node.open=true;
        animateBranchHeight(node,start,node.getBoundingClientRect().height);revealBranch(node);
      },()=>node.classList.remove('is-loading'));
      return;
    }
    const from=node.getBoundingClientRect().height;
    stopBranchAnimation(node);
    node.classList.toggle('branch-closing',!opening);
    if(opening)node.open=true;
    if(opening&&node.dataset.pending==='true')return;
    const to=opening?node.getBoundingClientRect().height:closedBranchHeight(node);
    animateBranchHeight(node,from,to,!opening);
  });
}
function resizeBranch(node,update){
  const before=node.getBoundingClientRect().height;
  for(let current=node;current;current=current.parentElement?.closest('details')){
    if(!current.classList.contains('branch-closing'))stopBranchAnimation(current);
  }
  update();
  if(node.open&&!node.classList.contains('branch-closing'))animateBranchHeight(node,before,node.getBoundingClientRect().height);
}
async function load() {
  closeAccountMenu();
  const request = ++loadNumber;
  const next = window.SNAPSHOT || (await api('workspaces/' + encodeURIComponent(ws)+'/board'));
  if (request !== loadNumber) return;
  data = next;
  data.objectTypes=next.object_types||{};
  $('timeline-link').href='/?workspace='+encodeURIComponent(ws)+'&view=timeline';
  $('tree-schema-link').href='/tree?workspace='+encodeURIComponent(ws);
  $('suggested-method-link').href='/guide?workspace='+encodeURIComponent(ws);
  await renderObjectTree();
}
async function showApp(identity) {
  closeAccountMenu();
  if(identity.email===undefined)identity={...identity,...await api('me')};
  viewerActor=identity.actor;
  $('identity').textContent=displayPersonName(identity.name||identity.email||identity.actor||'Account');
  $('identity').title=identity.email||'';
  document.body.classList.add('authenticated');
  $('logout').hidden=false;
  await list();
}
let signInAttempt = 0;
let signInWindow = null;
$('login-cancel').onclick=()=>{
  signInAttempt++;signInWindow?.close();$('login-button').disabled=false;
  $('login-cancel').hidden=true;$('login-status').textContent='';
};
$('login-button').onclick=async()=>{
  const attempt=++signInAttempt;
  const b=$('login-button'), status=$('login-status');b.disabled=true;status.dataset.error='false';
  status.textContent='Opening Google…';
  // Open synchronously from the click so browsers do not block the sign-in window.
  signInWindow=window.open('about:blank','joppa-google','popup,width=520,height=700');
  if(signInWindow)signInWindow.opener=null;
  try {
    const r=await fetch('/auth/login',{method:'POST'});const start=await r.json();
    if(!r.ok)throw Error(start.error||'Could not start sign-in.');
    if(attempt!==signInAttempt)return;
    if(!signInWindow)throw Error('Allow the Google sign-in window, then try again.');
    signInWindow.location=start.verification_url;
    status.textContent='Complete sign-in in the Google window.';$('login-cancel').hidden=false;
    const deadline=Date.now()+start.expires_in*1000;
    while(attempt===signInAttempt && Date.now()<deadline){
      await new Promise(resolve=>setTimeout(resolve,1800));
      if(attempt!==signInAttempt)return;
      const response=await fetch('/auth/poll',{method:'POST'}), result=await response.json();
      if(attempt!==signInAttempt)return;
      if(!response.ok)throw Error(result.error||'Sign-in failed. Please try again.');
      if(result.status==='ready'){signInWindow?.close();await showApp(result);return;}
      // Google may sever the popup reference via COOP; the server decides completion.
    }
    if(attempt===signInAttempt)throw Error('Sign-in expired. Please try again.');
  }catch(e){if(attempt===signInAttempt){status.textContent=e.message;status.dataset.error='true';signInWindow?.close();}}
  finally{if(attempt===signInAttempt){b.disabled=false;$('login-cancel').hidden=true;}}
};
$('logout').onclick=async()=>{await fetch('/auth/logout',{method:'POST'});sessionStorage.removeItem('joppa-token');location.reload();};
$('workspaces').onchange=e=>{
  ws=e.target.value;sessionStorage.setItem('joppa-workspace',ws);
  try{sessionStorage.setItem('joppa-workspace-title',e.target.selectedOptions[0]?.textContent||'');}catch{}
  history.pushState(null,'',objectURL());
  load().catch(notify);
};
$('refresh').onclick = () => load().catch(notify);
let statusHint=null;
function hideStatusHint(){if(statusHint)statusHint.hidden=true;}
function showStatusHint(circle){
  if(!statusHint){statusHint=E('div',undefined,'status-hint');statusHint.hidden=true;document.body.append(statusHint);}
  statusHint.textContent=circle.dataset.hint;
  statusHint.hidden=false;
  const dot=circle.getBoundingClientRect(),box=statusHint.getBoundingClientRect();
  const left=Math.max(8,Math.min(dot.left+dot.width/2-box.width/2,window.innerWidth-box.width-8));
  const below=dot.bottom+10;
  const top=below+box.height<=window.innerHeight-8?below:Math.max(8,dot.top-box.height-10);
  statusHint.style.left=left+'px';statusHint.style.top=top+'px';
}
document.addEventListener('scroll',hideStatusHint,true);
