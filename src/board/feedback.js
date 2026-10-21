// Messages to the person: the toast, error reports and buttons that report their errors.
import {E} from 'board/dom.js';
import {reducedMotion} from 'board/motion.js';
let toastTimer;
export function toast(message,failed=false){
  let host=document.querySelector('#toast');
  if(!host){host=E('div',undefined,'toast');host.id='toast';host.setAttribute('role','status');host.setAttribute('aria-live','polite');document.body.append(host);}
  host.textContent=message;
  host.classList.toggle('toast-failed',failed);
  host.classList.remove('toast-leaving');
  // Restart the clock so the newest message always gets its full three seconds.
  void host.offsetWidth;host.classList.add('toast-visible');
  clearTimeout(toastTimer);
  toastTimer=setTimeout(()=>{host.classList.remove('toast-visible');host.classList.add('toast-leaving');},reducedMotion()?3000:3180);
  return host;
}
export function notify(e) {
  toast(e?.message||String(e||''),e instanceof Error);
}
export function button(text, fn, cls = '') {
  const b = E('button', text, cls);
  b.type = 'button';
  b.onclick = () => {try{return Promise.resolve(fn()).catch(notify);}catch(e){notify(e);}};
  return b;
}
