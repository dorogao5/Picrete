"""S3 storage service for handling file uploads"""
import hashlib
import uuid
import base64
from datetime import datetime, timedelta
from io import BytesIO
from typing import BinaryIO, Optional

import aioboto3
from botocore.config import Config
from botocore.exceptions import ClientError
from fastapi import UploadFile

from app.core.config import settings


class S3StorageService:
    """Service for handling S3 file operations"""
    
    def __init__(self):
        self.session = aioboto3.Session()
        self.endpoint_url = settings.S3_ENDPOINT
        self.bucket_name = settings.S3_BUCKET
        self.region = settings.S3_REGION or "ru-central1"
        
        # S3 config for Yandex Object Storage
        # Yandex Object Storage uses virtual-hosted style addressing by default
        self.s3_config = Config(
            signature_version='s3v4',
            s3={'addressing_style': 'virtual'}  # Compatible with Yandex Object Storage
        )
    
    def _get_s3_client(self):
        """Get async S3 client"""
        return self.session.client(
            's3',
            endpoint_url=self.endpoint_url,
            aws_access_key_id=settings.S3_ACCESS_KEY,
            aws_secret_access_key=settings.S3_SECRET_KEY,
            region_name=self.region,
            config=self.s3_config
        )
    
    async def upload_file(
        self,
        file_content: bytes,
        filename: str,
        content_type: str,
        session_id: str,
        metadata: Optional[dict] = None
    ) -> tuple[str, int, str]:
        """
        Upload file to S3
        
        Args:
            file_content: File bytes
            filename: Original filename
            content_type: MIME type
            session_id: Exam session ID for organizing files
            metadata: Optional metadata to attach
            
        Returns:
            Tuple of (s3_key, file_size, file_hash)
        """
        # Generate unique file ID and key
        file_id = str(uuid.uuid4())
        file_extension = filename.split(".")[-1] if "." in filename else "jpg"
        s3_key = f"submissions/{session_id}/{file_id}.{file_extension}"
        
        # Calculate file hash
        file_hash = hashlib.sha256(file_content).hexdigest()
        
        # Prepare metadata (S3 metadata can only contain ASCII characters)
        # Encode filename to base64 to handle non-ASCII characters (e.g., Cyrillic)
        filename_encoded = base64.b64encode(filename.encode('utf-8')).decode('ascii')
        
        upload_metadata = {
            "original-filename-encoded": filename_encoded,
            "session-id": session_id,
            "file-hash": file_hash,
            "uploaded-at": datetime.utcnow().isoformat(),
        }
        if metadata:
            # Encode any non-ASCII metadata values
            encoded_metadata = {}
            for key, value in metadata.items():
                str_value = str(value)
                # Check if value contains non-ASCII characters
                if not str_value.isascii():
                    encoded_metadata[f"{key}-encoded"] = base64.b64encode(str_value.encode('utf-8')).decode('ascii')
                else:
                    encoded_metadata[key] = str_value
            upload_metadata.update(encoded_metadata)
        
        # Upload to S3
        async with self._get_s3_client() as s3:
            try:
                await s3.put_object(
                    Bucket=self.bucket_name,
                    Key=s3_key,
                    Body=file_content,
                    ContentType=content_type,
                    Metadata=upload_metadata,
                    # Security: prevent public access
                    ACL='private'
                )
            except ClientError as e:
                error_code = e.response.get('Error', {}).get('Code', 'Unknown')
                raise Exception(f"Failed to upload to S3: {error_code} - {str(e)}")
        
        return s3_key, len(file_content), file_hash
    
    async def upload_from_upload_file(
        self,
        upload_file: UploadFile,
        session_id: str,
        metadata: Optional[dict] = None
    ) -> tuple[str, int, str]:
        """
        Upload from FastAPI UploadFile object
        
        Returns:
            Tuple of (s3_key, file_size, file_hash)
        """
        content = await upload_file.read()
        await upload_file.seek(0)  # Reset for potential re-reading
        
        return await self.upload_file(
            file_content=content,
            filename=upload_file.filename or "image.jpg",
            content_type=upload_file.content_type or "image/jpeg",
            session_id=session_id,
            metadata=metadata
        )
    
    async def generate_presigned_url(
        self,
        s3_key: str,
        expires_in: int = None
    ) -> str:
        """
        Generate presigned URL for downloading file
        
        Args:
            s3_key: S3 object key
            expires_in: Expiration time in seconds (default from settings)
            
        Returns:
            Presigned URL
        """
        if expires_in is None:
            expires_in = settings.PRESIGNED_URL_EXPIRE_MINUTES * 60
        
        async with self._get_s3_client() as s3:
            try:
                url = await s3.generate_presigned_url(
                    'get_object',
                    Params={
                        'Bucket': self.bucket_name,
                        'Key': s3_key
                    },
                    ExpiresIn=expires_in
                )
                return url
            except ClientError as e:
                raise Exception(f"Failed to generate presigned URL: {str(e)}")
    
    async def generate_presigned_upload_url(
        self,
        session_id: str,
        filename: str,
        content_type: str,
        expires_in: int = None
    ) -> tuple[str, str]:
        """
        Generate presigned URL for direct upload from client
        
        Args:
            session_id: Exam session ID
            filename: Original filename
            content_type: MIME type
            expires_in: Expiration time in seconds
            
        Returns:
            Tuple of (presigned_url, s3_key)
        """
        if expires_in is None:
            expires_in = settings.PRESIGNED_URL_EXPIRE_MINUTES * 60
        
        # Generate unique key
        file_id = str(uuid.uuid4())
        file_extension = filename.split(".")[-1] if "." in filename else "jpg"
        s3_key = f"submissions/{session_id}/{file_id}.{file_extension}"
        
        async with self._get_s3_client() as s3:
            try:
                url = await s3.generate_presigned_url(
                    'put_object',
                    Params={
                        'Bucket': self.bucket_name,
                        'Key': s3_key,
                        'ContentType': content_type,
                    },
                    ExpiresIn=expires_in
                )
                return url, s3_key
            except ClientError as e:
                raise Exception(f"Failed to generate upload presigned URL: {str(e)}")
    
    async def delete_file(self, s3_key: str) -> bool:
        """
        Delete file from S3
        
        Args:
            s3_key: S3 object key
            
        Returns:
            True if successful
        """
        async with self._get_s3_client() as s3:
            try:
                await s3.delete_object(
                    Bucket=self.bucket_name,
                    Key=s3_key
                )
                return True
            except ClientError as e:
                raise Exception(f"Failed to delete from S3: {str(e)}")
    
    async def get_file(self, s3_key: str) -> bytes:
        """
        Get file content from S3
        
        Args:
            s3_key: S3 object key
            
        Returns:
            File content as bytes
        """
        async with self._get_s3_client() as s3:
            try:
                response = await s3.get_object(
                    Bucket=self.bucket_name,
                    Key=s3_key
                )
                async with response['Body'] as stream:
                    return await stream.read()
            except ClientError as e:
                if e.response.get('Error', {}).get('Code') == 'NoSuchKey':
                    raise FileNotFoundError(f"File not found: {s3_key}")
                raise Exception(f"Failed to get file from S3: {str(e)}")
    
    async def ensure_bucket_exists(self) -> bool:
        """
        Ensure the S3 bucket exists, create if it doesn't
        
        Returns:
            True if bucket exists or was created
        """
        async with self._get_s3_client() as s3:
            try:
                await s3.head_bucket(Bucket=self.bucket_name)
                return True
            except ClientError as e:
                error_code = e.response.get('Error', {}).get('Code')
                if error_code == '404':
                    # Bucket doesn't exist, try to create
                    try:
                        await s3.create_bucket(Bucket=self.bucket_name)
                        return True
                    except ClientError as create_error:
                        raise Exception(f"Failed to create bucket: {str(create_error)}")
                else:
                    raise Exception(f"Failed to check bucket: {str(e)}")


# Global instance
storage_service = S3StorageService()

