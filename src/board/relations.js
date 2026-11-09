// The relations of a card: one group for each relation key, named by the title of the registry
// in the tree schema. A group reads its edges with relations_of when the reader opens it, in
// pages of PAGE, and shows the condition `when` and the pinned revision of each edge.
import {state} from 'board/state.js';
import {E} from 'board/dom.js';
import {api,objectPart,PAGE} from 'board/api.js';
import {objectLink} from 'board/links.js';
import {notify} from 'board/feedback.js';
let registry={ws:null,titles:null};
// The title of each relation key of the workspace. One read of the tree for each workspace.
export function relationTitles(){
  if(registry.ws!==state.ws){
    const titles=api('workspaces/'+encodeURIComponent(state.ws)+'/tree').then(c=>Object.fromEntries(Object.entries(c.schema.relations||{}).map(([key,r])=>[key,r.title||key]))).catch(()=>({}));
    registry={ws:state.ws,titles};
  }
  return registry.titles;
}
// A group that reads the pages of `params` when it opens. `name` gives the summary from the
// title of the relation, and `row` gives the row of one item. The outgoing edges sit in
// `relation-edges`, so the depends_on list stays the one link list of a card without others.
export function pagedGroup(section,key,name,params,row,rowsClass='object-links'){
  const group=section(name(key),'object-relations link-group');group.dataset.relation=key;
  relationTitles().then(titles=>{group.querySelector('summary').textContent=name(titles[key]||key);});
  const rows=E('div',undefined,rowsClass);group.append(rows);
  // loaded: the first page arrived. loading: a request runs, so a second click waits.
  let loaded=false,loading=false;
  const load=async cursor=>{
    if(loading)return;loading=true;
    const more=group.querySelector('.load-more');if(more)more.disabled=true;
    try{
      const page=await objectPart({...params,limit:PAGE,...(cursor?{cursor}:{})});
      more?.remove();loaded=true;
      for(const item of page.items)rows.append(row(item));
      if(page.next_cursor){const next=E('button','Load more','text-button load-more');next.type='button';next.onclick=()=>load(page.next_cursor);group.append(next);}
    }catch(e){if(more)more.disabled=false;notify(e);}
    finally{loading=false;}
  };
  // A failed first page loads again when the reader opens the group again.
  group.addEventListener('toggle',()=>{if(group.open&&!loaded)load();});
  return group;
}
// A saved revision shows the edges of that revision from the read itself; the current card
// reads its pages with relations_of.
export function relationGroups(detail,section,historical){
  return Object.entries(detail.relations||{}).map(([key,relation])=>{
    const edges=relation.edges||[];
    const name=title=>title+(relation.title?' · '+relation.title:'')+' '+edges.length;
    const line=item=>{
      const edge=edges.find(e=>e.to===(item.address||item.to))||item;
      const address=item.address||item.to;
      const changed=item.revision&&edge.current_revision!==item.revision&&detail.review_required===true;
      const labels=[item.when,item.revision&&'revision '+item.revision,changed&&'changed',item.present===false&&'historical object',historical&&'current title'].filter(Boolean);
      const row=E('div',undefined,'relation-edge');row.append(objectLink(item.title||address,address,item.revision));
      if(labels.length)row.append(E('small',labels.join(' · '),'object-link-meta'));
      return row;
    };
    if(historical){
      const group=section(name(key),'object-relations link-group'),rows=E('div',undefined,'relation-edges');
      relationTitles().then(titles=>{group.querySelector('summary').textContent=name(titles[key]||key);});
      rows.append(...edges.map(line));group.append(rows);return group;
    }
    return pagedGroup(section,key,name,{relations_of:detail.address,direction:'outgoing',relation:key},line,'relation-edges');
  });
}
