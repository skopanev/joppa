// The incoming relation groups of a card. The object read gives a count for each relation and
// kind. A group reads its edges when the reader opens it, and Load more reads the next page.
import {E} from 'board/dom.js';
import {objectPart,PAGE} from 'board/api.js';
import {objectLink} from 'board/links.js';
import {notify} from 'board/feedback.js';
import {relationTitles} from 'board/relations.js';
export function incomingGroups(detail,section){
  return (detail.relation_groups||[]).filter(g=>g.direction==='incoming').map(g=>{
    const name=title=>'Linked from · '+title+' · '+g.kind+' '+g.count;
    const group=section(name(g.relation),'object-relations link-group');
    relationTitles().then(titles=>{group.querySelector('summary').textContent=name(titles[g.relation]||g.relation);});
    group.dataset.relation=g.relation;group.dataset.kind=g.kind;
    const rows=E('div',undefined,'object-links');group.append(rows);
    // loaded: the first page arrived. loading: a request runs, so a second click waits.
    let loaded=false,loading=false;
    const load=async cursor=>{
      if(loading)return;loading=true;
      const params={relations_of:detail.address,direction:'incoming',relation:g.relation,kind:g.kind,limit:PAGE};if(cursor)params.cursor=cursor;
      const more=group.querySelector('.load-more');if(more)more.disabled=true;
      try{
        const page=await objectPart(params);
        more?.remove();loaded=true;
        for(const item of page.items){const row=objectLink(item.title,item.address);row.append(E('small','Linked from · '+item.relation,'object-link-meta'));rows.append(row);}
        if(page.next_cursor){const next=E('button','Load more','text-button load-more');next.type='button';next.onclick=()=>load(page.next_cursor);group.append(next);}
      }catch(e){if(more)more.disabled=false;notify(e);}
      finally{loading=false;}
    };
    // A failed first page loads again when the reader opens the group again.
    group.addEventListener('toggle',()=>{if(group.open&&!loaded)load();});
    return group;
  });
}
