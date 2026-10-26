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
// While the promise of a click is pending, the button shows the spinner of the board lists.
// aria-busy tells assistive technology that the button waits for an answer.
export function busy(b, on) {
  b.setAttribute('aria-busy', String(on));
  b.querySelector(':scope > .spinner')?.remove();
  if (on) {const spin = E('span', undefined, 'spinner');spin.setAttribute('aria-hidden', 'true');b.prepend(spin);}
}
export function button(text, fn, cls = '') {
  const b = E('button', text, cls);
  b.type = 'button';
  b.onclick = () => {
    let result;
    try{result=fn();}catch(e){notify(e);return;}
    if(!(result instanceof Promise))return Promise.resolve(result);
    // A quick answer shows no spinner, so the button does not flicker.
    const timer=setTimeout(()=>busy(b,true),150);
    return result.catch(notify).finally(()=>{clearTimeout(timer);busy(b,false);});
  };
  return b;
}
