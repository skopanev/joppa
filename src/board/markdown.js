// Safe Markdown display of object text.
import {E} from 'board/dom.js';
// Object text is Markdown. Line breaks stay, active HTML is removed, and external links open safely.
const markdownSafe=(()=>{
  if(!window.DOMPurify||!window.marked)return null;
  DOMPurify.addHook('afterSanitizeAttributes',node=>{
    if(node.tagName==='A'){
      const href=node.getAttribute('href')||'';
      if(/^\s*(javascript|data|vbscript):/i.test(href))node.removeAttribute('href');
      else if(/^https?:/i.test(href)){node.setAttribute('target','_blank');node.setAttribute('rel','noopener noreferrer');}
    }
    if(node.tagName==='INPUT')node.setAttribute('disabled','');
  });
  return text=>DOMPurify.sanitize(marked.parse(text,{gfm:true,breaks:true}),
    {FORBID_TAGS:['style','iframe','svg','math','form','object','embed'],FORBID_ATTR:['style']});
})();
export function markdownBlock(text,cls){
  const box=E('div',undefined,'markdown '+cls);
  if(markdownSafe)box.innerHTML=markdownSafe(text);else{box.textContent=text;box.classList.add('markdown-raw');}
  return box;
}
