// The outgoing relations of a card: one group for each relation key, named by the title of the
// registry in the tree schema. A group shows its edges in pages of PAGE, with the condition
// `when` and the pinned revision of each edge.
import {state} from 'board/state.js';
import {E} from 'board/dom.js';
import {api,PAGE} from 'board/api.js';
import {objectLink} from 'board/links.js';
let registry={ws:null,titles:null};
// The title of each relation key of the workspace. One read of the tree for each workspace.
export function relationTitles(){
  if(registry.ws!==state.ws){
    const titles=api('workspaces/'+encodeURIComponent(state.ws)+'/tree').then(c=>Object.fromEntries(Object.entries(c.schema.relations||{}).map(([key,r])=>[key,r.title||key]))).catch(()=>({}));
    registry={ws:state.ws,titles};
  }
  return registry.titles;
}
export function relationGroups(detail,section,historical){
  return Object.entries(detail.relations||{}).map(([key,relation])=>{
    const edges=relation.edges||[];
    const name=title=>title+(relation.title?' · '+relation.title:'')+' '+edges.length;
    const group=section(name(key),'object-relations relation-group');group.dataset.relation=key;
    relationTitles().then(titles=>{group.querySelector('summary').textContent=name(titles[key]||key);});
    const rows=E('div',undefined,'object-links');group.append(rows);
    let shown=0;
    const more=()=>{
      for(const e of edges.slice(shown,shown+Number(PAGE))){
        const row=objectLink(e.title||e.to,e.to,e.revision);
        const changed=e.revision&&e.current_revision!==e.revision&&detail.review_required===true;
        const labels=[e.when,e.revision&&'revision '+e.revision,changed&&'changed',e.present===false&&'historical object',historical&&'current title'].filter(Boolean);
        if(labels.length)row.append(E('small',labels.join(' · '),'object-link-meta'));
        rows.append(row);
      }
      shown+=Number(PAGE);group.querySelector('.load-more')?.remove();
      if(shown<edges.length){const next=E('button','Load more','text-button load-more');next.type='button';next.onclick=more;group.append(next);}
    };
    more();
    return group;
  });
}
