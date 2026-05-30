class PathEditStore {
  editingPathId = $state<string | null>(null);
  selectedWaypointIds = $state<Set<string>>(new Set());

  begin(pathId: string): void {
    this.editingPathId = pathId;
    this.selectedWaypointIds = new Set();
  }

  end(pathId?: string): void {
    if (pathId !== undefined && this.editingPathId !== pathId) return;
    this.editingPathId = null;
    this.selectedWaypointIds = new Set();
  }

  setSelectedWaypointIds(ids: Iterable<string>): void {
    this.selectedWaypointIds = new Set(ids);
  }

  toggleWaypoint(id: string): void {
    const next = new Set(this.selectedWaypointIds);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    this.selectedWaypointIds = next;
  }
}

export const pathEditStore = new PathEditStore();
