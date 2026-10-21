// The open and close motion of tree branches.
const branchAnimations=new WeakMap();
export const reducedMotion=()=>window.matchMedia('(prefers-reduced-motion: reduce)').matches;
export function stopBranchAnimation(node){
  branchAnimations.get(node)?.cancel();
  branchAnimations.delete(node);
  node.style.height='';node.style.overflow='';
}
export function animateBranchHeight(node,from,to,closing=false){
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
export function enableBranchMotion(node){
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
export function resizeBranch(node,update){
  const before=node.getBoundingClientRect().height;
  for(let current=node;current;current=current.parentElement?.closest('details')){
    if(!current.classList.contains('branch-closing'))stopBranchAnimation(current);
  }
  update();
  if(node.open&&!node.classList.contains('branch-closing'))animateBranchHeight(node,before,node.getBoundingClientRect().height);
}
