// Apache-2.0. An isolated Jackcess adapter compiled with GraalVM Native Image.
// The installed application needs neither Java, Office, nor an ACE provider.
import java.io.*;
import java.math.BigDecimal;
import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import com.healthmarketscience.jackcess.*;
import com.healthmarketscience.jackcess.complex.*;
import com.healthmarketscience.jackcess.crypt.CryptCodecProvider;
import java.util.*;

public final class TomatoAccess {
    static void text(DataOutputStream out, String value) throws IOException {
        byte[] bytes=value.getBytes(StandardCharsets.UTF_8);
        if(bytes.length>16*1024*1024)throw new IOException("Access field exceeds 16 MiB");
        out.writeInt(bytes.length);out.write(bytes);
    }
    static String json(Object value) throws IOException {
        if(value==null)return "null";
        if(value instanceof byte[] bytes)return json(Base64.getEncoder().encodeToString(bytes));
        if(value instanceof ComplexValueForeignKey key)return json(key.getValues());
        if(value instanceof Attachment a){Map<String,Object> m=new LinkedHashMap<>();m.put("filename",a.getFileName());m.put("type",a.getFileType());m.put("url",a.getFileUrl());m.put("time",a.getFileTimeStampObject());m.put("flags",a.getFileFlags());m.put("base64",a.getFileData());return json(m);}
        if(value instanceof SingleValue v)return json(v.get());
        if(value instanceof Version v){Map<String,Object> m=new LinkedHashMap<>();m.put("value",v.getValue());m.put("modified",v.getModifiedDateObject());return json(m);}
        if(value instanceof UnsupportedValue v)return json(v.getValues());
        if(value instanceof Map<?,?> m){StringJoiner j=new StringJoiner(",","{","}");for(var e:m.entrySet())j.add(json(e.getKey().toString())+":"+json(e.getValue()));return j.toString();}
        if(value instanceof Iterable<?> list){StringJoiner j=new StringJoiner(",","[","]");for(Object v:list)j.add(json(v));return j.toString();}
        if(value instanceof Number||value instanceof Boolean)return value.toString();
        String s=value.toString();StringBuilder b=new StringBuilder("\"");for(char c:s.toCharArray())switch(c){case '"'->b.append("\\\"");case '\\'->b.append("\\\\");case '\n'->b.append("\\n");case '\r'->b.append("\\r");case '\t'->b.append("\\t");default->{if(c<32)b.append(String.format("\\u%04x",(int)c));else b.append(c);}}return b.append('"').toString();
    }
    static void read(String[] args) throws Exception {
        DataInputStream input=new DataInputStream(new BufferedInputStream(System.in));String password=text(input);
        try(Database db=new DatabaseBuilder(Path.of(args[1])).setReadOnly(true).setCodecProvider(new CryptCodecProvider(password)).open();DataOutputStream out=new DataOutputStream(new BufferedOutputStream(new FileOutputStream(args[2]),65536))) {
            db.setEvaluateExpressions(false);db.setDateTimeType(DateTimeType.LOCAL_DATE_TIME);
            if(args[0].equals("--list")){out.writeInt(0x54414c31);Set<String> names=db.getTableNames();out.writeInt(names.size());for(String name:names)text(out,name);return;}
            String name=text(input);Table table=db.getTable(name);if(table==null)throw new IOException("Access table not found");
            out.writeInt(0x54414931);out.writeInt(table.getColumnCount());for(Column c:table.getColumns()){text(out,c.getName());text(out,c.getType().name());}
            for(Row row:table){out.writeByte(1);for(Column column:table.getColumns()){
                Object value=row.get(column.getName());
                if(value==null){out.writeByte(0);}
                else if(value instanceof ComplexValueForeignKey){out.writeByte(1);text(out,json(value));}
                else if(value instanceof byte[] bytes){if(bytes.length>16*1024*1024)throw new IOException("Access binary field exceeds 16 MiB");out.writeByte(5);out.writeInt(bytes.length);out.write(bytes);}
                else if(value instanceof Boolean b){out.writeByte(4);out.writeBoolean(b);}
                else if(value instanceof Double||value instanceof Float){double n=((Number)value).doubleValue();if(!Double.isFinite(n))throw new IOException("Access numeric field is not finite");out.writeByte(3);out.writeDouble(n);}
                else {out.writeByte(1);String s=value instanceof java.time.LocalDateTime date?date.toString().replace('T',' '):value.toString();text(out,s);}
            }}out.writeByte(0);
        }
    }
    static String text(DataInputStream in) throws IOException {
        int n = in.readInt();
        if(n < 0 || n > 16 * 1024 * 1024) throw new IOException("Invalid field size");
        byte[] bytes = new byte[n]; in.readFully(bytes);
        return new String(bytes, StandardCharsets.UTF_8);
    }
    public static void main(String[] args) throws Exception {
        System.setProperty("org.apache.commons.logging.Log", "org.apache.commons.logging.impl.NoOpLog");
        if(args.length==3&&(args[0].equals("--list")||args[0].equals("--read"))){read(args);return;}
        if(args.length == 2 && args[0].equals("--verify")) {
            try(Database db = new DatabaseBuilder(Path.of(args[1])).setReadOnly(true).open()) {
                for(String name : db.getTableNames()) {
                    Table table = db.getTable(name);
                    long count=0; for(Row row:table) { count++; }
                    System.out.println(name + "\t" + count);
                }
            }
            return;
        }
        if(args.length != 1) throw new IOException("Expected output path");
        DataInputStream in = new DataInputStream(new BufferedInputStream(System.in, 65536));
        if(in.readInt() != 0x54414331) throw new IOException("Invalid export protocol");
        String name = text(in);
        int columns = in.readInt();
        if(columns < 1 || columns > 255) throw new IOException("Access supports 1..255 columns");
        TableBuilder builder = new TableBuilder(name);
        int[] types = new int[columns];
        for(int i=0;i<columns;i++) {
            String field=text(in); int type=in.readUnsignedByte(); types[i]=type;
            DataType target = switch(type) {
                case 1 -> DataType.NUMERIC; case 2 -> DataType.DOUBLE;
                case 3 -> DataType.BYTE; case 4 -> DataType.OLE;
                default -> DataType.MEMO;
            };
            ColumnBuilder column = new ColumnBuilder(field,target);
            if(type==1) column.setPrecision(28).setScale(0);
            builder.addColumn(column);
        }
        try(Database db = new DatabaseBuilder(Path.of(args[0])).setFileFormat(Database.FileFormat.V2007).setAutoSync(false).create()) {
            // Expressions are not used: values beginning with '=' stay literal text.
            db.setEvaluateExpressions(false);
            Table table=builder.toTable(db);
            long count=0;
            while(true) {
                int tag=in.read();
                if(tag == 0) break;
                if(tag != 1) throw new EOFException("Incomplete export stream");
                Object[] row=new Object[columns];
                for(int i=0;i<columns;i++) {
                    int kind=in.readUnsignedByte();
                    row[i]=switch(kind) {
                        case 0 -> null;
                        case 1 -> text(in);
                        case 2 -> new BigDecimal(text(in));
                        case 3 -> in.readDouble();
                        case 4 -> (byte)(in.readBoolean() ? 1 : 0);
                        case 5 -> { int n=in.readInt(); if(n<0||n>16*1024*1024)throw new IOException("Invalid binary field size"); byte[] b=new byte[n];in.readFully(b);yield b; }
                        default -> throw new IOException("Invalid cell type");
                    };
                }
                table.addRow(row);
                if((++count & 511)==0) db.flush();
            }
            db.flush();
            System.out.println(count);
        }
    }
}
