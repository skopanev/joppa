// Google sign-in, sign-out and the signed-in identity.
import {state} from 'board/state.js';
import {$} from 'board/dom.js';
import {closeAccountMenu} from 'board/account-menu.js';
import {api} from 'board/api.js';
import {list} from 'board/workspaces.js';
import {displayPersonName} from 'board/format.js';
export async function showApp(identity) {
  closeAccountMenu();
  if(identity.email===undefined)identity={...identity,...await api('me')};
  state.viewerActor=identity.actor;
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
