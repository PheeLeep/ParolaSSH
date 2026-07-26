import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  flexRender,
  getCoreRowModel,
  getFilteredRowModel,
  getPaginationRowModel,
  getSortedRowModel,
  useReactTable,
  type ColumnDef,
  type Row,
  type RowData,
  type RowSelectionState,
  type SortingState,
  type VisibilityState,
} from "@tanstack/react-table";
import { Button, Dropdown, Form, InputGroup, Table } from "react-bootstrap";
import {
  ArrowDownWideNarrow,
  ArrowUpDown,
  ArrowUpNarrowWide,
  ChevronLeft,
  ChevronRight,
  ChevronsLeft,
  ChevronsRight,
  Columns3,
  Inbox,
  Search,
  X,
  type LucideIcon,
} from "lucide-react";

declare module "@tanstack/react-table" {
  interface ColumnMeta<TData extends RowData, TValue> {
    /** Extra classes for the `<th>`. */
    headerClassName?: string;
    /** Extra classes for the `<td>`. */
    cellClassName?: string;
    /** Fixed column width, e.g. `"12rem"`. */
    width?: string;
    /** Human label for the column-visibility menu, when the header is not plain text. */
    title?: string;
    /** Pin the column to an edge while the rest of the table scrolls. */
    sticky?: "left" | "right";
  }
}

function stickyClass(sticky: "left" | "right" | undefined, extra?: string) {
  if (!sticky) return extra;
  const pinned = `datatable__sticky datatable__sticky--${sticky}`;
  return extra ? `${pinned} ${extra}` : pinned;
}

const SELECT_COLUMN_ID = "__select";

type DataTableProps<TData> = {
  data: TData[];
  columns: ColumnDef<TData, any>[];
  /** Stable row id — important so selection survives sorting and filtering. */
  getRowId?: (row: TData, index: number) => string;
  enableRowSelection?: boolean;
  onSelectionChange?: (rows: TData[]) => void;
  /** Double-click / Enter on a row. */
  onRowActivate?: (row: TData) => void;
  searchPlaceholder?: string;
  /** Buttons rendered on the left of the toolbar. */
  toolbarActions?: ReactNode;
  emptyMessage?: string;
  initialPageSize?: number;
  className?: string;
};

export function DataTable<TData>({
  data,
  columns,
  getRowId,
  enableRowSelection = false,
  onSelectionChange,
  onRowActivate,
  searchPlaceholder = "Search…",
  toolbarActions,
  emptyMessage = "Nothing to show yet.",
  initialPageSize = 10,
  className,
}: DataTableProps<TData>) {
  const [sorting, setSorting] = useState<SortingState>([]);
  const [globalFilter, setGlobalFilter] = useState("");
  const [columnVisibility, setColumnVisibility] = useState<VisibilityState>({});
  const [rowSelection, setRowSelection] = useState<RowSelectionState>({});

  const resolvedColumns = useMemo<ColumnDef<TData, any>[]>(() => {
    if (!enableRowSelection) return columns;

    const selectColumn: ColumnDef<TData, any> = {
      id: SELECT_COLUMN_ID,
      enableSorting: false,
      enableHiding: false,
      enableGlobalFilter: false,
      meta: {
        width: "2.75rem",
        sticky: "left",
        headerClassName: "text-center",
        cellClassName: "text-center",
      },
      header: ({ table }) => (
        <SelectCheckbox
          checked={table.getIsAllPageRowsSelected()}
          indeterminate={table.getIsSomePageRowsSelected()}
          onChange={table.getToggleAllPageRowsSelectedHandler()}
          label="Select all rows on this page"
        />
      ),
      cell: ({ row }) => (
        <SelectCheckbox
          checked={row.getIsSelected()}
          disabled={!row.getCanSelect()}
          onChange={row.getToggleSelectedHandler()}
          label="Select row"
        />
      ),
    };

    return [selectColumn, ...columns];
  }, [columns, enableRowSelection]);

  const table = useReactTable({
    data,
    columns: resolvedColumns,
    state: { sorting, globalFilter, columnVisibility, rowSelection },
    getRowId,
    enableRowSelection,
    onSortingChange: setSorting,
    onGlobalFilterChange: setGlobalFilter,
    onColumnVisibilityChange: setColumnVisibility,
    onRowSelectionChange: setRowSelection,
    getCoreRowModel: getCoreRowModel(),
    getFilteredRowModel: getFilteredRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getPaginationRowModel: getPaginationRowModel(),
    initialState: { pagination: { pageSize: initialPageSize } },
  });

  // Report selection upward without making the parent own the state.
  const selectedRows = table.getSelectedRowModel().rows;
  useEffect(() => {
    onSelectionChange?.(selectedRows.map((row) => row.original));
    // `selectedRows` is a fresh array each render; key off the selection state.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rowSelection, data]);

  const hideableColumns = table
    .getAllLeafColumns()
    .filter((column) => column.getCanHide());

  const { pageIndex, pageSize } = table.getState().pagination;
  const filteredCount = table.getFilteredRowModel().rows.length;
  const firstRow = filteredCount === 0 ? 0 : pageIndex * pageSize + 1;
  const lastRow = Math.min((pageIndex + 1) * pageSize, filteredCount);

  return (
    <div className={className}>
      <div className="d-flex flex-wrap align-items-center gap-2 mb-3">
        {toolbarActions}

        <InputGroup size="sm" className="ms-auto" style={{ maxWidth: "18rem" }}>
          <InputGroup.Text>
            <Search className="icon-sm" aria-hidden="true" />
          </InputGroup.Text>
          <Form.Control
            value={globalFilter}
            onChange={(event) => setGlobalFilter(event.target.value)}
            placeholder={searchPlaceholder}
            aria-label={searchPlaceholder}
          />
          {globalFilter && (
            <Button
              variant="outline-secondary"
              onClick={() => setGlobalFilter("")}
              aria-label="Clear search"
            >
              <X className="icon-sm" aria-hidden="true" />
            </Button>
          )}
        </InputGroup>

        <Dropdown align="end" autoClose="outside">
          <Dropdown.Toggle variant="outline-secondary" size="sm" id="column-visibility">
            <Columns3 aria-hidden="true" />
            Columns
          </Dropdown.Toggle>
          <Dropdown.Menu>
            {hideableColumns.map((column) => (
              <div key={column.id} className="px-3 py-1">
                <Form.Check
                  type="checkbox"
                  id={`toggle-${column.id}`}
                  label={columnLabel(column.id, column.columnDef.meta?.title)}
                  checked={column.getIsVisible()}
                  onChange={column.getToggleVisibilityHandler()}
                />
              </div>
            ))}
          </Dropdown.Menu>
        </Dropdown>
      </div>

      <div className="table-responsive border rounded datatable__scroll">
        <Table hover className="mb-0 align-middle">
          {/* No `.table-light` here — it paints an inset box-shadow that would
              cover the themed header background from app.css. */}
          <thead>
            {table.getHeaderGroups().map((headerGroup) => (
              <tr key={headerGroup.id}>
                {headerGroup.headers.map((header) => {
                  const meta = header.column.columnDef.meta;
                  const sortable = header.column.getCanSort();
                  const sorted = header.column.getIsSorted();

                  return (
                    <th
                      key={header.id}
                      scope="col"
                      style={meta?.width ? { width: meta.width } : undefined}
                      className={stickyClass(meta?.sticky, meta?.headerClassName)}
                      aria-sort={
                        sorted === "asc"
                          ? "ascending"
                          : sorted === "desc"
                            ? "descending"
                            : undefined
                      }
                    >
                      {header.isPlaceholder ? null : sortable ? (
                        <button
                          type="button"
                          className="btn btn-link btn-sm p-0 text-decoration-none text-body fw-semibold"
                          onClick={header.column.getToggleSortingHandler()}
                        >
                          {flexRender(header.column.columnDef.header, header.getContext())}
                          <SortIcon sorted={sorted} />
                        </button>
                      ) : (
                        flexRender(header.column.columnDef.header, header.getContext())
                      )}
                    </th>
                  );
                })}
              </tr>
            ))}
          </thead>

          <tbody>
            {table.getRowModel().rows.length === 0 ? (
              <tr>
                <td
                  colSpan={table.getVisibleLeafColumns().length}
                  className="text-center text-body-secondary py-5"
                >
                  <Inbox className="icon-xl mb-2" aria-hidden="true" />
                  <div>
                    {globalFilter ? `No matches for “${globalFilter}”.` : emptyMessage}
                  </div>
                </td>
              </tr>
            ) : (
              table.getRowModel().rows.map((row) => (
                <TableRow key={row.id} row={row} onActivate={onRowActivate} />
              ))
            )}
          </tbody>
        </Table>
      </div>

      <div className="d-flex flex-wrap align-items-center gap-3 mt-3">
        <span className="text-body-secondary small">
          {filteredCount === 0
            ? "No rows"
            : `Showing ${firstRow}–${lastRow} of ${filteredCount}`}
          {enableRowSelection && selectedRows.length > 0 && (
            <> · {selectedRows.length} selected</>
          )}
        </span>

        <Form.Select
          size="sm"
          className="w-auto"
          value={pageSize}
          onChange={(event) => table.setPageSize(Number(event.target.value))}
          aria-label="Rows per page"
        >
          {[10, 25, 50, 100].map((size) => (
            <option key={size} value={size}>
              {size} / page
            </option>
          ))}
        </Form.Select>

        <div className="btn-group btn-group-sm ms-auto" role="group" aria-label="Pagination">
          <Button
            variant="outline-secondary"
            onClick={() => table.firstPage()}
            disabled={!table.getCanPreviousPage()}
            aria-label="First page"
          >
            <ChevronsLeft aria-hidden="true" />
          </Button>
          <Button
            variant="outline-secondary"
            onClick={() => table.previousPage()}
            disabled={!table.getCanPreviousPage()}
            aria-label="Previous page"
          >
            <ChevronLeft aria-hidden="true" />
          </Button>
          <Button variant="outline-secondary" disabled style={{ pointerEvents: "none" }}>
            {pageIndex + 1} / {Math.max(table.getPageCount(), 1)}
          </Button>
          <Button
            variant="outline-secondary"
            onClick={() => table.nextPage()}
            disabled={!table.getCanNextPage()}
            aria-label="Next page"
          >
            <ChevronRight aria-hidden="true" />
          </Button>
          <Button
            variant="outline-secondary"
            onClick={() => table.lastPage()}
            disabled={!table.getCanNextPage()}
            aria-label="Last page"
          >
            <ChevronsRight aria-hidden="true" />
          </Button>
        </div>
      </div>
    </div>
  );
}

function TableRow<TData>({
  row,
  onActivate,
}: {
  row: Row<TData>;
  onActivate?: (row: TData) => void;
}) {
  return (
    <tr
      className={row.getIsSelected() ? "table-active" : undefined}
      onDoubleClick={onActivate ? () => onActivate(row.original) : undefined}
      onKeyDown={
        onActivate
          ? (event) => {
              if (event.key === "Enter") onActivate(row.original);
            }
          : undefined
      }
      tabIndex={onActivate ? 0 : undefined}
      style={onActivate ? { cursor: "pointer" } : undefined}
    >
      {row.getVisibleCells().map((cell) => {
        const meta = cell.column.columnDef.meta;
        return (
          <td key={cell.id} className={stickyClass(meta?.sticky, meta?.cellClassName)}>
            {flexRender(cell.column.columnDef.cell, cell.getContext())}
          </td>
        );
      })}
    </tr>
  );
}

function SelectCheckbox({
  checked,
  indeterminate = false,
  disabled = false,
  onChange,
  label,
}: {
  checked: boolean;
  indeterminate?: boolean;
  disabled?: boolean;
  onChange: (event: unknown) => void;
  label: string;
}) {
  const ref = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (ref.current) ref.current.indeterminate = !checked && indeterminate;
  }, [checked, indeterminate]);

  return (
    <input
      ref={ref}
      type="checkbox"
      className="form-check-input m-0"
      checked={checked}
      disabled={disabled}
      onChange={onChange}
      aria-label={label}
      onClick={(event) => event.stopPropagation()}
    />
  );
}

/** Unsorted columns keep the affordance visible but dimmed. */
function SortIcon({ sorted }: { sorted: false | "asc" | "desc" }) {
  const Icon: LucideIcon =
    sorted === "asc"
      ? ArrowUpNarrowWide
      : sorted === "desc"
        ? ArrowDownWideNarrow
        : ArrowUpDown;

  return (
    <Icon className={`icon-sm ${sorted ? "" : "opacity-25"}`} aria-hidden="true" />
  );
}

/** `lastConnected` -> "Last Connected" for the column menu. */
function columnLabel(id: string, title?: string) {
  if (title) return title;
  return id
    .replace(/([A-Z])/g, " $1")
    .replace(/^./, (char) => char.toUpperCase())
    .trim();
}
