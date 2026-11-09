// The incoming relation groups of a card. The object read gives a count for each relation and
// kind. A group reads its edges when the reader opens it, and Load more reads the next page.
import {E} from 'board/dom.js';
import {objectLink} from 'board/links.js';
import {pagedGroup} from 'board/relations.js';
export function incomingGroups(detail,section){
  return (detail.relation_groups||[]).filter(g=>g.direction==='incoming').map(g=>{
    const name=title=>'Linked from · '+title+' · '+g.kind+' '+g.count;
    const params={relations_of:detail.address,direction:'incoming',relation:g.relation,kind:g.kind};
    const group=pagedGroup(section,g.relation,name,params,item=>{const row=objectLink(item.title,item.address);row.append(E('small','Linked from · '+item.relation,'object-link-meta'));return row;});
    group.dataset.kind=g.kind;
    return group;
  });
}
