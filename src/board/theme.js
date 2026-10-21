// The light and dark theme switch.
import {$} from 'board/dom.js';
const theme=$('theme');
theme.onclick=()=>{
  const next=darkTheme()?'light':'dark';
  document.documentElement.dataset.theme=next;
  try{localStorage.setItem('theme',next);}catch{}
  updateThemeColor();
};
const systemTheme=window.matchMedia('(prefers-color-scheme: dark)');
function darkTheme(){
  const selected=document.documentElement.dataset.theme;
  return selected==='dark'||selected==='system'&&systemTheme.matches;
}
function updateThemeColor(){
  const dark=darkTheme();
  theme.dataset.dark=String(dark);
  theme.title=dark?'Switch to light theme':'Switch to dark theme';
  theme.setAttribute('aria-label',theme.title);
  document.querySelector('meta[name="theme-color"]').content=dark?'#191c20':'#fffcf5';
}
systemTheme.addEventListener('change',updateThemeColor);
updateThemeColor();
