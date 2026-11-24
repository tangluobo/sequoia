/* SPDX-License-Identifier: MIT
 * Read-only 32-bit ODBC bridge. OS driver manager is not redistributed.
 * Credentials use stdin. Diagnostics never include driver-supplied messages.
 */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <sql.h>
#include <sqlext.h>
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>
#include <io.h>
#include <fcntl.h>
#define LIMIT (16u*1024u*1024u)
static void die(const char *message){fprintf(stderr,"ODBC: %s\n",message);exit(1);}
static void ok(SQLRETURN r,const char *step){if(!SQL_SUCCEEDED(r))die(step);}
static void *alloc(size_t n){void *p=malloc(n?n:1);if(!p)die("out of memory");return p;}
static uint32_t input32(void){unsigned char b[4];if(fread(b,1,4,stdin)!=4)die("incomplete input");return ((uint32_t)b[0]<<24)|((uint32_t)b[1]<<16)|((uint32_t)b[2]<<8)|b[3];}
static void output32(uint32_t n){unsigned char b[4]={(unsigned char)(n>>24),(unsigned char)(n>>16),(unsigned char)(n>>8),(unsigned char)n};if(fwrite(b,1,4,stdout)!=4)die("output unavailable");}
static wchar_t *input_text(void){uint32_t n=input32();if(n>1024*1024)die("input string too large");char *s=alloc(n+1);if(fread(s,1,n,stdin)!=n)die("incomplete input");s[n]=0;int count=MultiByteToWideChar(CP_UTF8,MB_ERR_INVALID_CHARS,s,(int)n,NULL,0);if(n&&!count)die("invalid UTF-8 input");wchar_t *w=alloc(((size_t)count+1)*sizeof(wchar_t));if(count&&!MultiByteToWideChar(CP_UTF8,MB_ERR_INVALID_CHARS,s,(int)n,w,count))die("invalid input");w[count]=0;SecureZeroMemory(s,n);free(s);return w;}
static void output_text(const wchar_t *s,size_t chars){if(chars>LIMIT/2)die("field exceeds 16 MiB");int n=WideCharToMultiByte(CP_UTF8,WC_ERR_INVALID_CHARS,s,(int)chars,NULL,0,NULL,NULL);if(chars&&!n)die("invalid UTF-16 from driver");char *b=alloc(n);if(n&&!WideCharToMultiByte(CP_UTF8,WC_ERR_INVALID_CHARS,s,(int)chars,b,n,NULL,NULL))die("invalid UTF-16 from driver");output32((uint32_t)n);if(fwrite(b,1,n,stdout)!=(size_t)n)die("output unavailable");free(b);}
typedef struct {unsigned char *data;size_t length;int null;} Value;
static Value get(SQLHSTMT stmt,SQLUSMALLINT col,int binary){Value v={0};size_t capacity=0;unsigned char chunk[8192];
    for(;;){SQLLEN indicator=0;memset(chunk,0,sizeof(chunk));SQLRETURN r=SQLGetData(stmt,col,binary?SQL_C_BINARY:SQL_C_WCHAR,chunk,sizeof(chunk),&indicator);if(r==SQL_NO_DATA)break;ok(r,"cannot retrieve field");if(indicator==SQL_NULL_DATA){v.null=1;return v;}
        size_t payload=sizeof(chunk)-(binary?0:sizeof(wchar_t));int more=r==SQL_SUCCESS_WITH_INFO&&(indicator==SQL_NO_TOTAL||indicator>(SQLLEN)payload);
        size_t n=more?payload:(indicator>=0?(size_t)indicator:(binary?0:wcsnlen((wchar_t*)chunk,payload/2)*2));if(n>payload)die("driver returned invalid field length");if(v.length+n>LIMIT)die("field exceeds 16 MiB");
        if(v.length+n>capacity){capacity=v.length+n;v.data=realloc(v.data,capacity?capacity:1);if(!v.data)die("out of memory");}if(n)memcpy(v.data+v.length,chunk,n);v.length+=n;if(!more)break;if(!n)die("driver made no progress");
    }return v;
}
static void out_field(SQLHSTMT stmt,SQLUSMALLINT col,int binary,size_t *row_bytes){Value v=get(stmt,col,binary);*row_bytes+=v.length;if(*row_bytes>LIMIT)die("record exceeds 16 MiB");if(v.null)fputc(0,stdout);else if(binary){fputc(5,stdout);output32((uint32_t)v.length);if(fwrite(v.data,1,v.length,stdout)!=v.length)die("output unavailable");}else{fputc(1,stdout);output_text((wchar_t*)v.data,v.length/2);}free(v.data);}
static void metadata_text(SQLHSTMT stmt,SQLUSMALLINT col){Value v=get(stmt,col,0);output_text((wchar_t*)v.data,v.length/2);free(v.data);}
static void list_sources(SQLHENV env){SQLWCHAR name[4096],description[16384];SQLSMALLINT n,d;SQLUSMALLINT direction=SQL_FETCH_FIRST;SQLRETURN r;output32(0x544f5331);
    while(SQL_SUCCEEDED(r=SQLDataSourcesW(env,direction,name,4096,&n,description,16384,&d))){direction=SQL_FETCH_NEXT;if(n<0||n>=4096)die("data source name too long");fputc(1,stdout);output_text(name,n);}if(r!=SQL_NO_DATA)ok(r,"cannot enumerate data sources");direction=SQL_FETCH_FIRST;
    while(SQL_SUCCEEDED(r=SQLDriversW(env,direction,name,4096,&n,description,16384,&d))){direction=SQL_FETCH_NEXT;if(n<0||n>=4096)die("driver name too long");fputc(2,stdout);output_text(name,n);}if(r!=SQL_NO_DATA)ok(r,"cannot enumerate drivers");fputc(0,stdout);
}
static SQLHDBC connect(SQLHENV env){SQLHDBC dbc;ok(SQLAllocHandle(SQL_HANDLE_DBC,env,&dbc),"cannot allocate connection");ok(SQLSetConnectAttr(dbc,SQL_LOGIN_TIMEOUT,(SQLPOINTER)(uintptr_t)10,0),"cannot set login timeout");
    wchar_t *dsn=input_text(),*user=input_text(),*password=input_text(),*str=input_text();SQLRETURN r;
    if(*str)r=SQLDriverConnectW(dbc,NULL,str,SQL_NTS,NULL,0,NULL,SQL_DRIVER_NOPROMPT);else r=SQLConnectW(dbc,dsn,SQL_NTS,user,SQL_NTS,password,SQL_NTS);
    SecureZeroMemory(password,wcslen(password)*2);SecureZeroMemory(str,wcslen(str)*2);free(password);free(str);free(dsn);free(user);ok(r,"connection failed; check driver, source and credentials");
    /* Drivers may reject a read-only hint; all actual statements are reads. */
    SQLSetConnectAttr(dbc,SQL_ATTR_ACCESS_MODE,(SQLPOINTER)(uintptr_t)SQL_MODE_READ_ONLY,0);return dbc;
}
static void list_tables(SQLHSTMT stmt){ok(SQLTablesW(stmt,NULL,0,NULL,0,NULL,0,L"'TABLE','VIEW'",SQL_NTS),"cannot enumerate source tables");output32(0x544f5431);SQLRETURN r;unsigned count=0;
    while(SQL_SUCCEEDED(r=SQLFetch(stmt))){if(++count>10000)die("too many source tables");fputc(1,stdout);metadata_text(stmt,1);metadata_text(stmt,2);metadata_text(stmt,3);}if(r!=SQL_NO_DATA)ok(r,"cannot fetch source tables");fputc(0,stdout);
}
static wchar_t *quote_name(const wchar_t *s,wchar_t first){wchar_t last=first==L'['?L']':first;size_t n=wcslen(s);if(!n||n>65535)die("invalid source table name");wchar_t *out=alloc((2*n+3)*2),*p=out;*p++=first;for(;*s;s++){*p++=*s;if(*s==last)*p++=*s;}*p++=last;*p=0;return out;}
static wchar_t *table_query(SQLHDBC dbc){wchar_t *catalog=input_text(),*schema=input_text(),*table=input_text();SQLWCHAR quote[8]={L'"',0};SQLSMALLINT length=0;SQLGetInfoW(dbc,SQL_IDENTIFIER_QUOTE_CHAR,quote,sizeof(quote),&length);wchar_t q=quote[0]>32?quote[0]:L'"';
    wchar_t *parts[3]={0};size_t size=32;int count=0;
    if(*catalog&&!wcschr(catalog,L'\\')&&!wcschr(catalog,L'/')&&!wcschr(catalog,L':'))parts[count++]=quote_name(catalog,q);if(*schema)parts[count++]=quote_name(schema,q);parts[count++]=quote_name(table,q);
    for(int i=0;i<count;i++)size+=wcslen(parts[i])+1;wchar_t *sql=alloc(size*2);wcscpy_s(sql,size,L"SELECT * FROM ");for(int i=0;i<count;i++){if(i)wcscat_s(sql,size,L".");wcscat_s(sql,size,parts[i]);free(parts[i]);}free(catalog);free(schema);free(table);return sql;
}
static void read_query(SQLHSTMT stmt,const wchar_t *sql){SQLSetStmtAttr(stmt,SQL_ATTR_QUERY_TIMEOUT,(SQLPOINTER)(uintptr_t)30,0);ok(SQLExecDirectW(stmt,(SQLWCHAR*)sql,SQL_NTS),"source query failed");SQLSMALLINT cols;ok(SQLNumResultCols(stmt,&cols),"cannot describe source");if(cols<1||cols>16384)die("invalid source column count");int *binary=calloc(cols,sizeof(int));if(!binary)die("out of memory");output32(0x544f5231);output32((uint32_t)cols);
    for(SQLUSMALLINT i=1;i<=(SQLUSMALLINT)cols;i++){SQLWCHAR name[4096];SQLSMALLINT n,type,scale,nullable;SQLULEN size;ok(SQLDescribeColW(stmt,i,name,4096,&n,&type,&size,&scale,&nullable),"cannot describe field");if(n<0||n>=4096)die("field name too long");output_text(name,n);binary[i-1]=type==SQL_BINARY||type==SQL_VARBINARY||type==SQL_LONGVARBINARY;output32((uint32_t)(int32_t)type);output32((uint32_t)size);output32((uint32_t)(int32_t)scale);}
    SQLRETURN r;while(SQL_SUCCEEDED(r=SQLFetch(stmt))){fputc(1,stdout);size_t bytes=0;for(SQLUSMALLINT i=1;i<=(SQLUSMALLINT)cols;i++)out_field(stmt,i,binary[i-1],&bytes);}if(r!=SQL_NO_DATA)ok(r,"source fetch failed");fputc(0,stdout);free(binary);
}
int main(void){_setmode(_fileno(stdin),_O_BINARY);_setmode(_fileno(stdout),_O_BINARY);if(input32()!=0x544f4431)die("invalid protocol");uint32_t op=input32();SQLHENV env;ok(SQLAllocHandle(SQL_HANDLE_ENV,SQL_NULL_HANDLE,&env),"cannot allocate environment");ok(SQLSetEnvAttr(env,SQL_ATTR_ODBC_VERSION,(SQLPOINTER)(uintptr_t)SQL_OV_ODBC3,0),"cannot initialize environment");
    if(op==1){list_sources(env);}else{SQLHDBC dbc=connect(env);SQLHSTMT stmt;ok(SQLAllocHandle(SQL_HANDLE_STMT,dbc,&stmt),"cannot allocate statement");if(op==2)list_tables(stmt);else if(op==3||op==4){wchar_t *sql=op==4?table_query(dbc):input_text();read_query(stmt,sql);free(sql);}else die("invalid operation");SQLFreeHandle(SQL_HANDLE_STMT,stmt);SQLDisconnect(dbc);SQLFreeHandle(SQL_HANDLE_DBC,dbc);}SQLFreeHandle(SQL_HANDLE_ENV,env);if(fflush(stdout))die("output unavailable");return 0;
}
