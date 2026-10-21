// The account menu: it closes on an outside click, on Escape and on navigation.
import {$} from 'board/dom.js';
export function closeAccountMenu(){$('account-menu').open=false;}
document.addEventListener('click',event=>{if(!$('account-menu').contains(event.target))closeAccountMenu();},true);
// A long name wraps the menu to the left edge of a narrow screen. Then the options open to the right.
// The click sets the side before the menu opens, so the first frame of the open menu is on screen.
$('identity').addEventListener('click',()=>{const right=$('account-menu').getBoundingClientRect().right;document.querySelector('.account-options').classList.toggle('to-right',right<Math.min(190,innerWidth-32));});
window.addEventListener('pageshow',closeAccountMenu);
document.addEventListener('keydown',event=>{
  if(event.key==='Escape'&&$('account-menu').open){$('account-menu').open=false;$('identity').focus();}
});
