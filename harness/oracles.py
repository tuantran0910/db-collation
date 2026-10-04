"""Live-database oracles. Used ONLY by the differential harness (test time),
never by the db-collation library itself.
"""

import contextlib
import time

from .model import OrderResult


class SourceUnsupported(Exception):
    """The source database cannot create this collation (e.g. a server too old
    to support a feature such as ICU `rules`). This is an expected skip, not a
    harness failure."""


class NullOracle:
    pass


class PostgresOracle:
    def __init__(
        self,
        host="127.0.0.1",
        port=18432,
        user="postgres",
        password="cdc-review",
        dbname="postgres",
    ):
        import psycopg  # local import: harness-only dependency

        self.psycopg = psycopg
        self.conn = psycopg.connect(
            f"host={host} port={port} user={user} password={password} dbname={dbname}",
            autocommit=True,
        )

    def reset_table(self, corpus):
        cur = self.conn.cursor()
        cur.execute("DROP TABLE IF EXISTS harness_corpus")
        cur.execute("CREATE TABLE harness_corpus(id int primary key, s text)")
        with self.conn.cursor() as c:
            c.executemany(
                "INSERT INTO harness_corpus(id, s) VALUES (%s, %s)",
                [(sc.id, sc.s) for sc in corpus],
            )

    def order(self, spec, corpus):
        name = "harness_" + spec.id.replace("-", "_")
        cur = self.conn.cursor()
        cur.execute(f'DROP COLLATION IF EXISTS "{name}"')
        # CREATE COLLATION is DDL: no bind parameters, so inline quoted literals.
        locale = spec.collation.replace("'", "''")
        det = "true" if spec.deterministic else "false"
        extra = ""
        if spec.rules:
            extra += ", rules='" + spec.rules.replace("'", "''") + "'"
        try:
            cur.execute(
                f'CREATE COLLATION "{name}" '
                f"(provider=icu, locale='{locale}', deterministic={det}{extra})"
            )
        except self.psycopg.errors.UndefinedObject as exc:
            # Older servers (PG < 16) do not know the `rules` parameter.
            raise SourceUnsupported(str(exc).splitlines()[0]) from exc
        except self.psycopg.errors.SyntaxError as exc:
            msg = str(exc)
            if spec.rules and ("rules" in msg or "not recognized" in msg):
                raise SourceUnsupported(msg.splitlines()[0]) from exc
            raise
        # The ICU-locale column: PG14 stored it in collcollate, PG15/16 used
        # colliculocale, PG17+ renamed it colllocale. Probe the catalog.
        cols = {
            r[0]
            for r in cur.execute(
                "SELECT column_name FROM information_schema.columns WHERE table_name='pg_collation'"
            )
        }
        loc_col = (
            "colliculocale"
            if "colliculocale" in cols
            else "colllocale"
            if "colllocale" in cols
            else "collcollate"
        )
        meta = cur.execute(
            f"SELECT {loc_col}, collversion, pg_collation_actual_version(oid) "
            f"FROM pg_collation WHERE collname=%s",
            (name,),
        ).fetchone()
        rows = cur.execute(
            f'SELECT id, dense_rank() OVER (ORDER BY s COLLATE "{name}") '
            f'FROM harness_corpus ORDER BY s COLLATE "{name}", id'
        ).fetchall()
        order = [r[0] for r in rows]
        rank = {r[0]: int(r[1]) for r in rows}
        # Use the *actual* version the server reports now (meta[2]); on a fresh
        # CREATE COLLATION it equals the stored collversion (meta[1]). This is
        # what the candidate must reproduce.
        return OrderResult(
            spec_id=spec.id,
            order=order,
            rank=rank,
            version=meta[2],
            supported=True,
            note=str(meta[0]),
        )

    def close(self):
        self.conn.close()


class OracleOracle:
    """Live Oracle oracle via `python-oracledb` in thin mode.

    Oracle selects collation by name through `NLSSORT(s, 'NLS_SORT=<name>')`.
    The corpus is loaded into a table and ordered per spec with a window
    `DENSE_RANK()`, giving both the total order and the equivalence classes.

    Only three `NLS_SORT` values are modelled by the crate; the rest are
    refused there (and asserted refused by the matrix), but the oracle still
    orders them so the refusal is *against real data*, not an assumption.
    """

    def __init__(
        self,
        host="127.0.0.1",
        port=1521,
        service="FREEPDB1",
        user="system",
        password="cdc-review",
    ):
        import oracledb  # local import: harness-only dependency

        self.oracledb = oracledb
        self.conn = oracledb.connect(user=user, password=password, dsn=f"{host}:{port}/{service}")
        cur = self.conn.cursor()
        # Oracle has no `TRUNCATE` reset story worth relying on across versions;
        # drop-and-create is deterministic.
        with contextlib.suppress(oracledb.DatabaseError):
            cur.execute("DROP TABLE harness_corpus")
        cur.execute("CREATE TABLE harness_corpus (id NUMBER PRIMARY KEY, s VARCHAR2(4000 CHAR))")
        self.conn.commit()

    def reset_table(self, corpus):
        cur = self.conn.cursor()
        cur.execute("TRUNCATE TABLE harness_corpus")
        cur.executemany(
            "INSERT INTO harness_corpus(id, s) VALUES (:1, :2)",
            [(int(sc.id), sc.s) for sc in corpus],
        )
        self.conn.commit()

    def order(self, spec, corpus):
        cur = self.conn.cursor()
        # `NLS_SORT` is a collation name, not a bind-able value inside the
        # `NLSSORT` format string, so the name must be inlined. Names come from
        # the fixed matrix, never from user input; still, reject anything that
        # could break out of the quoted literal.
        name = spec.collation
        if not all(c.isalnum() or c == "_" for c in name):
            raise ValueError(f"unsafe Oracle NLS_SORT name: {name!r}")
        cur.execute(
            f"SELECT id, DENSE_RANK() OVER "
            f"(ORDER BY NLSSORT(s, 'NLS_SORT={name}')) "
            f"FROM harness_corpus"
        )
        rows = cur.fetchall()
        rank = {int(r[0]): int(r[1]) for r in rows}
        order = [int(r[0]) for r in sorted(rows, key=lambda r: (r[1], r[0]))]
        # Oracle identifies the active UCA data version through the `NLS_SORT`
        # name (`UCA1210` = Unicode 12.1, `UCA0700` = Unicode 7.0); `BINARY` has
        # none. The canonical string is the Unicode version the crate reports.
        version = {
            "UCA1210": "12.1.0",
            "UCA0700": "7.0.0",
        }.get(name[:7])
        return OrderResult(
            spec_id=spec.id,
            order=order,
            rank=rank,
            version=version,
            supported=True,
            note=name,
        )

    def close(self):
        self.conn.close()


class MySqlOracle:
    def __init__(
        self, host="127.0.0.1", port=18306, user="root", password="cdc-review", db="harness"
    ):
        import pymysql  # local import: harness-only dependency

        self.conn = pymysql.connect(
            host=host, port=port, user=user, password=password, charset="utf8mb4", autocommit=True
        )
        cur = self.conn.cursor()
        cur.execute(f"DROP DATABASE IF EXISTS {db}")
        cur.execute(f"CREATE DATABASE {db} CHARACTER SET utf8mb4")
        self.db = db
        cur.execute(f"USE {db}")
        cur.execute("CREATE TABLE t(id INT PRIMARY KEY, s VARCHAR(1000) CHARACTER SET utf8mb4)")

    def reset_table(self, corpus):
        cur = self.conn.cursor()
        cur.execute(f"USE {self.db}")
        cur.execute("TRUNCATE TABLE t")
        cur.executemany("INSERT INTO t(id, s) VALUES (%s, %s)", [(sc.id, sc.s) for sc in corpus])

    def order(self, spec, corpus):
        cur = self.conn.cursor()
        cur.execute(f"USE {self.db}")
        cur.execute(
            f"SELECT id, DENSE_RANK() OVER (ORDER BY s COLLATE {spec.collation}) "
            f"FROM t ORDER BY s COLLATE {spec.collation}, id"
        )
        rows = cur.fetchall()
        order = [r[0] for r in rows]
        rank = {r[0]: int(r[1]) for r in rows}
        return OrderResult(spec_id=spec.id, order=order, rank=rank, supported=True)

    def close(self):
        self.conn.close()


def wait_for_port(host, port, timeout=60.0):
    import socket

    end = time.time() + timeout
    while time.time() < end:
        try:
            with socket.create_connection((host, port), timeout=1):
                return True
        except OSError:
            time.sleep(0.5)
    return False
